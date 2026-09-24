use crate::index::LineIndex;
use crate::parse::{self, Encoding};
use crate::source::Source;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 컬럼 하나에 거는 엑셀 자동필터 스타일 조건. 두 조건은 함께 있으면 AND다.
/// - `contains`: 부분 문자열 포함(대소문자 무시). 비어 있으면 조건 없음.
/// - `included`: 체크박스로 고른 "포함할 값" 집합. `None`이면 값 목록 조건 없음
///   (엑셀의 "전체 선택" 상태와 같다). `Some(set)`이면 이 집합에 있는 값만 통과.
#[derive(Debug, Clone, Default)]
pub struct ColumnFilter {
    pub contains: String,
    pub included: Option<HashSet<String>>,
}

impl ColumnFilter {
    /// 아무 조건도 없는지(= 이 컬럼에 필터가 없는 것과 같은지).
    pub fn is_noop(&self) -> bool {
        self.contains.trim().is_empty() && self.included.is_none()
    }
}

/// 필터를 전체 데이터에 적용한 결과. `matched`는 조건을 통과한 데이터 행의
/// **원본 논리 행번호**를 오름차순으로 담는다. 이 결과를 만든 조건 자체는
/// `Document::column_filters`가 원본으로 들고 있으므로 여기 다시 담지 않는다.
#[derive(Debug, Clone)]
pub struct FilterState {
    pub matched: Vec<u32>,
}

/// 컬럼의 고유값 총 개수. `EXACT_COUNT_CAP` 이내면 정확한 값(전수 카운트),
/// 넘으면 `HyperLogLog`로 낸 근사값(원소 자체를 저장하지 않아 메모리가
/// 고정이다 — `HyperLogLog` 타입 주석 참조).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistinctCount {
    Exact(usize),
    Approx(u64),
}

/// 드롭다운에 보여줄 고유값 목록과 총 개수. 목록은 카디널리티가 너무 크면
/// 잘라내고 `truncated`를 세운다 — 자유 텍스트 컬럼(예: 전부 다른 ID)에서
/// 수백만 체크박스를 그리면 UI가 죽으므로, 그런 컬럼은 "포함" 텍스트 필터로
/// 좁히라고 안내한다.
pub struct DistinctResult {
    pub values: Vec<(String, u64)>,
    pub truncated: bool,
    pub count: DistinctCount,
}

/// 드롭다운에 실제로 그릴 고유값 개수 상한.
///
/// 체크박스 목록은 표와 달리 가상 스크롤이 없다 — 드롭다운이 열려 있는 동안
/// 매 프레임 이 개수만큼 위젯을 전부 새로 그린다. 그래서 표(수억 행도 거뜬한)
/// 보다 훨씬 낮게 잡는다. 검색창(`filter_menu_search`)이 이 목록을 실시간으로
/// 좁혀 주므로, 그 이상 필요하면 타이핑으로 줄이면 된다.
pub const MAX_DISTINCT_VALUES: usize = 1_000;

/// "고유값 총 개수"를 정확히 셀지(전수) 근사(HyperLogLog)로 낼지 가르는 경계.
/// 이 개수 이내면 스캔 중 누적하는 해시맵에 실제로 값을 다 담아 정확히 센다.
/// 넘으면(=ID처럼 전부 다른 값인 컬럼) 더 이상 새 값을 담지 않고 `HyperLogLog`
/// 추정치로 전환한다 — 그래야 카디널리티가 아무리 커도 메모리가 이 상수
/// 크기로 고정된다(`HyperLogLog` 참조, 원소를 하나도 저장하지 않는다).
const EXACT_COUNT_CAP: usize = 5_000;

/// `HyperLogLog`의 레지스터 개수를 정하는 지수. 레지스터 `2^HLL_P`개, 각
/// 1바이트 — 총 메모리는 `2^HLL_P`바이트로 고정(값이 몇 종류든 안 바뀐다).
/// 14로 두면 16,384개 레지스터(16KB)에 표준오차 약 1.04/√16384 ≈ 0.8%.
const HLL_P: u32 = 14;
const HLL_M: usize = 1 << HLL_P;

/// 원소를 하나도 저장하지 않고 "서로 다른 값이 대략 몇 개인지"를 추정하는
/// 카운터(HyperLogLog). 각 값을 해시해 상위 `HLL_P`비트로 레지스터를 고르고,
/// 나머지 비트의 "선행 0 개수"를 그 레지스터의 최댓값으로 기록한다 — 선행 0이
/// 길게 나오는 건 드문 일이라(2^k분의 1), 그런 값이 관찰됐다는 사실 자체가
/// "지금까지 대략 2^k개의 서로 다른 값을 봤다"는 신호가 된다. 레지스터
/// `HLL_M`개로 이 추정을 독립적으로 반복해(각 값은 해시로 무작위 레지스터에
/// 배정되므로) 평균 내면 오차가 크게 줄어든다.
///
/// 메모리는 항상 `HLL_M`바이트 — 100개를 셌든 1억 개를 셌든 크기가 안 변한다.
/// 그 대신 정확한 개수가 아니라 근사치를 낸다.
struct HyperLogLog {
    registers: Vec<u8>,
}

impl HyperLogLog {
    fn new() -> Self {
        Self { registers: vec![0u8; HLL_M] }
    }

    fn insert(&mut self, value: &str) {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        let hash = hasher.finish();
        let idx = (hash >> (64 - HLL_P)) as usize;
        // 레지스터를 고르는 데 쓴 상위 HLL_P비트를 빼고 나머지로 선행 0을 센다.
        // 그 비트들을 맨 앞으로 밀어 올린 뒤 leading_zeros를 쓰면 된다.
        let rest = hash << HLL_P;
        let zeros = (rest.leading_zeros() + 1) as u8;
        if zeros > self.registers[idx] {
            self.registers[idx] = zeros;
        }
    }

    fn merge(&mut self, other: &HyperLogLog) {
        for i in 0..HLL_M {
            if other.registers[i] > self.registers[i] {
                self.registers[i] = other.registers[i];
            }
        }
    }

    /// 표준 HyperLogLog 추정 공식(Flajolet et al. 2007).
    ///
    /// **소규모 보정(linear counting)이 왜 필요한가.** 처음엔 "레지스터가
    /// 16384개나 되니 생략해도 된다"고 판단했는데 틀렸다 — 조화평균 공식은
    /// 레지스터 중 상당수가 아직 한 번도 안 채워졌을 때(원소 수가 레지스터
    /// 수에 비해 적을 때) 위로 치우친 값을 낸다. 이 앱에서 근사치를 쓰는
    /// 구간은 `EXACT_COUNT_CAP`(5,000) 초과인데, 레지스터가 16384개면
    /// "소규모"의 기준(`2.5 * m` = 40,960)이 그보다 훨씬 커서 실제 동작
    /// 구간(5천~4만) 전체가 이 보정이 필요한 영역이었다(테스트로 실측:
    /// 10,000개 추정에서 17,281이 나와 70% 넘게 틀렸다). 그래서 빈 레지스터
    /// 비율만으로 셈하는 linear counting을 그 구간에 쓴다 — 빈 레지스터가
    /// 많을수록(원소가 적을수록) 이 쪽이 훨씬 정확하다.
    fn estimate(&self) -> u64 {
        let m = HLL_M as f64;
        let alpha = 0.7213 / (1.0 + 1.079 / m);
        let sum: f64 = self.registers.iter().map(|&r| 2f64.powi(-(r as i32))).sum();
        let raw = alpha * m * m / sum;

        let zeros = self.registers.iter().filter(|&&r| r == 0).count();
        let estimate = if raw <= 2.5 * m && zeros > 0 {
            m * (m / zeros as f64).ln()
        } else {
            raw
        };
        estimate.round() as u64
    }
}

/// 스캔 워커의 순차 처리 단위. `sort.rs`와 같은 값 — mmap 순차 접근으로
/// prefetch 친화적이게 만든다.
const CHUNK: usize = 64 * 1024;

/// 뒤쪽 CR/LF만 제거한 슬라이스(`sort.rs`의 동명 함수와 동일 — 모듈이 갈려
/// 공유하지 않는다. 한 줄짜리 로직이라 의존을 만들 만큼의 가치가 없다).
fn trim_newline(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b'\n' || b[end - 1] == b'\r') {
        end -= 1;
    }
    &b[..end]
}

/// 필드 raw 바이트 → 화면에 보이는 값(따옴표 벗김 + 디코딩).
///
/// `parse::field_slice`는 속도를 위해 인용 부호를 슬라이스에 그대로 남긴다
/// (`sort.rs`의 정렬 키가 같은 사정으로 quote를 안 벗기는 것과 동일). 체크박스
/// 목록이나 포함 비교는 셀에 실제로 보이는 값과 일치해야 하므로, 여기서 한 번
/// 정규화한다. 멀티라인 인용 필드(필드 안에 실제 개행이 든 경우)는 애초에
/// `field_slice`가 줄 단위로만 보므로 다루지 않는다 — 정렬 키 추출과 같은
/// 한계이고, 이 앱에서 이미 감수하고 있는 근사치다.
fn field_display_value(bytes: &[u8], enc: Encoding) -> String {
    let s = parse::decode_line(bytes, enc);
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].replace("\"\"", "\"")
    } else {
        s
    }
}

/// 한 행의 선택 컬럼 값을 화면 표시용 문자열로 얻는다(락 없는 hot path).
/// 컬럼이 없는 행은 빈 문자열로 취급한다(정렬의 `empty_key`와 같은 정책).
fn row_field_value(
    bytes: &[u8],
    offsets: &[u64],
    total_bytes: u64,
    enc: Encoding,
    delim: u8,
    col: usize,
    logical: usize,
) -> String {
    let Some((s, e)) = LineIndex::range_in(offsets, total_bytes, logical) else {
        return String::new();
    };
    let raw = &bytes[s as usize..e as usize];
    match enc {
        Encoding::Utf8 | Encoding::Cp949 => {
            let line = trim_newline(raw);
            match parse::field_slice(line, delim, col) {
                Some(field) => field_display_value(field, enc),
                None => String::new(),
            }
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let text = parse::decode_line(raw, enc);
            let fields = parse::split_fields(text.trim_end_matches(['\r', '\n']), delim);
            fields.get(col).cloned().unwrap_or_default()
        }
    }
}

/// `row_field_value`의 편집 버퍼(인메모리) 버전. 편집 버퍼는 이미 디코딩된
/// UTF-8 `String` 줄이라(`crate::edit::load_edit_buffer`) 인코딩 분기가
/// 필요 없다 — 그래서 mmap 경로처럼 `Encoding`을 받지 않는다.
fn line_field_value(line: &str, delim: u8, col: usize) -> String {
    match parse::field_slice(line.as_bytes(), delim, col) {
        Some(field) => field_display_value(field, Encoding::Utf8),
        None => String::new(),
    }
}

/// `extract_distinct`의 편집 버퍼(인메모리) 버전. 편집 모드는 이미 파일
/// 전체를 동기적으로 메모리에 올려 두는 구조라(`enter_edit_mode` 주석)
/// 배경 스레드 없이 그 자리에서 바로 계산한다 — `apply_edit_sort`가 정렬을
/// 동기로 처리하는 것과 같은 전제다.
pub fn extract_distinct_lines(lines: &[String], delim: u8, col: usize, data_start: usize) -> DistinctResult {
    if lines.len() <= data_start {
        return DistinctResult { values: Vec::new(), truncated: false, count: DistinctCount::Exact(0) };
    }
    let mut map: HashMap<String, u64> = HashMap::new();
    let mut hll = HyperLogLog::new();
    for line in &lines[data_start..] {
        let v = line_field_value(line, delim, col);
        hll.insert(&v);
        capped_bump(&mut map, v, EXACT_COUNT_CAP);
    }
    let count = if map.len() < EXACT_COUNT_CAP {
        DistinctCount::Exact(map.len())
    } else {
        DistinctCount::Approx(hll.estimate())
    };
    let hit_cap = map.len() >= EXACT_COUNT_CAP;
    let mut values: Vec<(String, u64)> = map.into_iter().collect();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    let truncated = hit_cap || values.len() > MAX_DISTINCT_VALUES;
    values.truncate(MAX_DISTINCT_VALUES);
    DistinctResult { values, truncated, count }
}

/// `apply_filters`의 편집 버퍼(인메모리) 버전. 필터 조건을 통과한 데이터
/// 행의 논리 행번호(편집 버퍼 인덱스)를 오름차순으로 반환한다.
pub fn apply_filters_lines(
    lines: &[String],
    filters: &[(usize, ColumnFilter)],
    delim: u8,
    data_start: usize,
) -> Vec<u32> {
    if lines.len() <= data_start {
        return Vec::new();
    }
    if filters.is_empty() {
        return (data_start..lines.len()).map(|l| l as u32).collect();
    }
    let compiled = compile(filters);
    (data_start..lines.len())
        .into_par_iter()
        .filter(|&logical| {
            compiled.iter().all(|c| {
                let value = line_field_value(&lines[logical], delim, c.col);
                if !c.contains_lower.is_empty() && !value.to_lowercase().contains(&c.contains_lower) {
                    return false;
                }
                if let Some(set) = &c.included {
                    if !set.contains(&value) {
                        return false;
                    }
                }
                true
            })
        })
        .map(|l| l as u32)
        .collect()
}

/// 선택 컬럼의 고유값과 등장 횟수를 병렬로 추출한다. 청크마다 로컬
/// `HashMap`을 만들어 세고 마지막에 병합한다(`sort.rs`의 청크 순차 순회와
/// 같은 이유 — mmap prefetch 친화적).
pub fn extract_distinct(
    source: &Arc<Source>,
    index: &LineIndex,
    enc: Encoding,
    delim: u8,
    col: usize,
    data_start: usize,
    progress: Option<&(dyn Fn(usize) + Sync)>,
) -> DistinctResult {
    let total = index.line_count();
    if total <= data_start {
        return DistinctResult { values: Vec::new(), truncated: false, count: DistinctCount::Exact(0) };
    }
    let data_rows = total - data_start;
    let (offsets, total_bytes) = index.snapshot();
    let offsets: &[u64] = &offsets;
    let bytes = source.as_bytes();

    let n_chunks = data_rows.div_ceil(CHUNK);
    // 청크마다 (상한 걸린 해시맵, HyperLogLog 스케치)를 함께 만든다. 해시맵은
    // `EXACT_COUNT_CAP`(5,000)을 넘으면 새 값을 안 담지만, HLL은 원소를 저장하지
    // 않으므로 상한 없이 전체 행을 계속 반영한다 — 그래서 정확 카운트가 상한을
    // 넘어가도(=더 이상 정확하지 않게 돼도) HLL 쪽 근사치는 계속 정확도가 오른다.
    let (merged, hll): (HashMap<String, u64>, HyperLogLog) = (0..n_chunks)
        .into_par_iter()
        .map(|chunk_idx| {
            let base = data_start + chunk_idx * CHUNK;
            let end = (base + CHUNK).min(total);
            let mut local: HashMap<String, u64> = HashMap::new();
            let mut local_hll = HyperLogLog::new();
            for logical in base..end {
                let v = row_field_value(bytes, offsets, total_bytes, enc, delim, col, logical);
                local_hll.insert(&v);
                // 청크 하나(최대 CHUNK=64K행)는 어차피 그 크기로 자연히
                // 제한되지만, 여기서도 상한을 적용해 두면 이미 상한에 닿은
                // 청크는 새 키 삽입 없이 카운트만 갱신하며 더 빨리 지나간다.
                capped_bump(&mut local, v, EXACT_COUNT_CAP);
            }
            if let Some(p) = progress {
                p(end - base);
            }
            (local, local_hll)
        })
        // rayon의 `reduce`는 트리 형태로 여러 스레드에서 동시에 호출될 수 있어
        // `Fn`이어야 한다(외부 변수를 캡처해 갱신하는 `FnMut`는 못 쓴다) — 그래서
        // "잘렸는지"는 여기서 플래그로 표시하지 않고, 병합이 끝난 뒤
        // `merged.len() >= EXACT_COUNT_CAP`로 판단한다(상한에 정확히 닿았을
        // 때만 우연히 안 잘렸는데 잘렸다고 뜨는 극단적 경계 케이스가 있을 수
        // 있지만, 그 정도 근사는 안내 문구 성격상 문제 되지 않는다).
        .reduce(
            || (HashMap::new(), HyperLogLog::new()),
            |(mut a, mut a_hll), (b, b_hll)| {
                // **핵심 방어선**: 청크가 수천 개인 대용량 파일에서 이 reduce가
                // 트리 형태로 전부 합쳐지므로, 여기서 막지 않으면 ID처럼 전부
                // 다른 값인 컬럼은 합쳐진 맵이 파일 전체 행 수만큼 자란다(그게
                // 원래 사용자가 걱정한 "버벅임"의 실제 원인이었다). 상한에
                // 닿으면 이미 있는 키의 카운트만 더하고 새 키는 버린다 —
                // 메모리·시간이 파일 크기와 무관하게 `EXACT_COUNT_CAP`으로
                // 고정된다.
                for (k, v) in b {
                    if let Some(c) = a.get_mut(&k) {
                        *c += v;
                    } else if a.len() < EXACT_COUNT_CAP {
                        a.insert(k, v);
                    }
                }
                a_hll.merge(&b_hll);
                (a, a_hll)
            },
        );

    let count = if merged.len() < EXACT_COUNT_CAP {
        // 상한에 안 닿았다 = 스캔 도중 버려진 새 값이 없었다 = 이게 진짜 정확한
        // 개수다.
        DistinctCount::Exact(merged.len())
    } else {
        DistinctCount::Approx(hll.estimate())
    };
    let hit_scan_cap = merged.len() >= EXACT_COUNT_CAP;
    let mut values: Vec<(String, u64)> = merged.into_iter().collect();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    let truncated = hit_scan_cap || values.len() > MAX_DISTINCT_VALUES;
    values.truncate(MAX_DISTINCT_VALUES);
    DistinctResult { values, truncated, count }
}

/// 상한 내에서만 새 키를 추가하고, 이미 있는 키는 상한과 무관하게 카운트를
/// 올린다(청크 로컬 누적용 — `extract_distinct`의 reduce 상한과 같은 정책).
fn capped_bump(map: &mut HashMap<String, u64>, key: String, cap: usize) {
    if let Some(c) = map.get_mut(&key) {
        *c += 1;
    } else if map.len() < cap {
        map.insert(key, 1);
    }
}

/// 조건을 미리 컴파일한 형태 — 스캔 루프 안에서 `to_lowercase()`를 조건마다
/// 매 행 반복하지 않도록 필요한 값(소문자화된 needle)을 한 번만 만든다.
struct Compiled {
    col: usize,
    contains_lower: String,
    included: Option<HashSet<String>>,
}

fn compile(filters: &[(usize, ColumnFilter)]) -> Vec<Compiled> {
    filters
        .iter()
        .map(|(col, f)| Compiled {
            col: *col,
            contains_lower: f.contains.to_lowercase(),
            included: f.included.clone(),
        })
        .collect()
}

fn row_passes(
    bytes: &[u8],
    offsets: &[u64],
    total_bytes: u64,
    enc: Encoding,
    delim: u8,
    compiled: &[Compiled],
    logical: usize,
) -> bool {
    compiled.iter().all(|c| {
        let value = row_field_value(bytes, offsets, total_bytes, enc, delim, c.col, logical);
        if !c.contains_lower.is_empty() && !value.to_lowercase().contains(&c.contains_lower) {
            return false;
        }
        if let Some(set) = &c.included {
            if !set.contains(&value) {
                return false;
            }
        }
        true
    })
}

/// 필터 조건들을 전체 데이터에 적용해 통과한 행의 논리 행번호를 오름차순으로
/// 반환한다. 여러 컬럼 조건은 AND로 묶인다. `filters`가 비어 있으면 전체 행이
/// 통과한다(호출측은 보통 조건이 있을 때만 이 함수를 부른다).
pub fn apply_filters(
    source: &Arc<Source>,
    index: &LineIndex,
    enc: Encoding,
    delim: u8,
    filters: &[(usize, ColumnFilter)],
    data_start: usize,
    progress: Option<&(dyn Fn(usize) + Sync)>,
) -> Vec<u32> {
    let total = index.line_count();
    if total <= data_start {
        return Vec::new();
    }
    if filters.is_empty() {
        return (data_start..total).map(|l| l as u32).collect();
    }
    let (offsets, total_bytes) = index.snapshot();
    let offsets: &[u64] = &offsets;
    let bytes = source.as_bytes();
    let compiled = compile(filters);

    let data_rows = total - data_start;
    let n_chunks = data_rows.div_ceil(CHUNK);
    let chunks: Vec<Vec<u32>> = (0..n_chunks)
        .into_par_iter()
        .map(|chunk_idx| {
            let base = data_start + chunk_idx * CHUNK;
            let end = (base + CHUNK).min(total);
            let mut local = Vec::new();
            for logical in base..end {
                if row_passes(bytes, offsets, total_bytes, enc, delim, &compiled, logical) {
                    local.push(logical as u32);
                }
            }
            if let Some(p) = progress {
                p(end - base);
            }
            local
        })
        .collect();

    let mut matched = Vec::with_capacity(data_rows / 4);
    for chunk in chunks {
        matched.extend(chunk);
    }
    matched
}

// ---- 백그라운드 작업 인프라(`sort.rs`의 `SortJob`과 같은 패턴) ----

struct DistinctShared {
    rows_done: AtomicU64,
    total_rows: u64,
    result: Mutex<Option<DistinctResult>>,
    finished: AtomicBool,
}

/// 백그라운드 고유값 추출 작업 핸들. 어느 컬럼 것인지는 호출측
/// (`Document::distinct_values_col`)이 별도로 들고 있으므로 여기 담지 않는다.
pub struct DistinctJob {
    shared: Arc<DistinctShared>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl DistinctJob {
    pub fn progress(&self) -> f32 {
        let total = self.shared.total_rows;
        if total == 0 {
            return 1.0;
        }
        let done = self.shared.rows_done.load(Ordering::Relaxed);
        (done as f32 / total as f32).clamp(0.0, 1.0)
    }

    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }

    pub fn take_result(&mut self) -> Option<DistinctResult> {
        if !self.is_finished() {
            return None;
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.shared.result.lock().unwrap().take()
    }
}

pub fn spawn_distinct_values(
    source: Arc<Source>,
    index: LineIndex,
    enc: Encoding,
    delim: u8,
    col: usize,
    data_start: usize,
    ctx: egui::Context,
) -> DistinctJob {
    let total_rows = (index.line_count().saturating_sub(data_start)) as u64;
    let shared = Arc::new(DistinctShared {
        rows_done: AtomicU64::new(0),
        total_rows,
        result: Mutex::new(None),
        finished: AtomicBool::new(false),
    });

    let shared_bg = shared.clone();
    let handle = std::thread::spawn(move || {
        let progress = {
            let shared = shared_bg.clone();
            let ctx = ctx.clone();
            move |n: usize| {
                shared.rows_done.fetch_add(n as u64, Ordering::Relaxed);
                ctx.request_repaint();
            }
        };
        let result = extract_distinct(&source, &index, enc, delim, col, data_start, Some(&progress));
        shared_bg.rows_done.store(shared_bg.total_rows, Ordering::Relaxed);
        *shared_bg.result.lock().unwrap() = Some(result);
        shared_bg.finished.store(true, Ordering::Relaxed);
        ctx.request_repaint();
    });

    DistinctJob { shared, handle: Some(handle) }
}

struct FilterShared {
    rows_done: AtomicU64,
    total_rows: u64,
    result: Mutex<Option<Vec<u32>>>,
    finished: AtomicBool,
}

/// 백그라운드 필터 적용 작업 핸들.
pub struct FilterJob {
    shared: Arc<FilterShared>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// 이 작업이 필터와 **함께** 적용한 정렬 기준(없으면 빈 배열). 완료 후
    /// 호출측이 헤더 화살표 등 표시용 `SortState`를 이 값으로 채운다 —
    /// 실제 정렬은 이미 `matched`(반환값) 순서에 반영돼 있다.
    pub specs: Vec<crate::sort::SortSpec>,
}

impl FilterJob {
    pub fn progress(&self) -> f32 {
        let total = self.shared.total_rows;
        if total == 0 {
            return 1.0;
        }
        let done = self.shared.rows_done.load(Ordering::Relaxed);
        (done as f32 / total as f32).clamp(0.0, 1.0)
    }

    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }

    pub fn take_result(&mut self) -> Option<Vec<u32>> {
        if !self.is_finished() {
            return None;
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.shared.result.lock().unwrap().take()
    }
}

/// 필터 조건을 적용하고, `sort_specs`가 있으면 그 결과를 **그 자리에서 마저
/// 정렬**한다(엑셀에서 필터+정렬을 같이 쓰는 것과 같은 동작 — 필터로 추린
/// 행들 안에서만 정렬 순서가 매겨진다, 파일 전체를 다시 정렬하지 않는다).
/// 한 배경 스레드 안에서 필터 → 정렬을 순차로 처리한다: 정렬 대상이 이미
/// 필터를 통과한 (보통 훨씬 작은) 부분집합이라 두 단계를 각각 따로 스레드로
/// 띄우는 것보다 조율이 간단하고, 필터만 있고 정렬이 없으면(`sort_specs`가
/// 비어 있으면) 정렬 단계 자체를 건너뛴다.
pub fn spawn_apply_filters(
    source: Arc<Source>,
    index: LineIndex,
    enc: Encoding,
    delim: u8,
    filters: Vec<(usize, ColumnFilter)>,
    sort_specs: Vec<crate::sort::SortSpec>,
    data_start: usize,
    ctx: egui::Context,
) -> FilterJob {
    let total_rows = (index.line_count().saturating_sub(data_start)) as u64;
    let shared = Arc::new(FilterShared {
        rows_done: AtomicU64::new(0),
        total_rows,
        result: Mutex::new(None),
        finished: AtomicBool::new(false),
    });

    let shared_bg = shared.clone();
    let specs_bg = sort_specs.clone();
    let handle = std::thread::spawn(move || {
        let progress = {
            let shared = shared_bg.clone();
            let ctx = ctx.clone();
            move |n: usize| {
                shared.rows_done.fetch_add(n as u64, Ordering::Relaxed);
                ctx.request_repaint();
            }
        };
        let matched = apply_filters(&source, &index, enc, delim, &filters, data_start, Some(&progress));
        // 정렬 기준이 있으면 필터링된 부분집합만 마저 정렬한다. 진행률
        // 콜백은 필터 단계(보통 지배적인 비용 — 전체 파일을 훑는 쪽)에만
        // 붙인다; 정렬 단계는 이미 걸러진 작은 집합이라 굳이 진행률을 또
        // 보고할 만큼 오래 걸리지 않는다.
        let matched = if specs_bg.is_empty() {
            matched
        } else {
            crate::sort::extract_and_multi_sort_subset(
                &source, &index, enc, delim, &specs_bg, &matched, None,
            )
        };
        shared_bg.rows_done.store(shared_bg.total_rows, Ordering::Relaxed);
        *shared_bg.result.lock().unwrap() = Some(matched);
        shared_bg.finished.store(true, Ordering::Relaxed);
        ctx.request_repaint();
    });

    FilterJob { shared, handle: Some(handle), specs: sort_specs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source;
    use std::io::Write;

    fn temp(content: &[u8]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("tv_filter_{}_{}.csv", std::process::id(), id));
        std::fs::File::create(&p).unwrap().write_all(content).unwrap();
        p
    }

    fn open_indexed(content: &[u8]) -> (Arc<Source>, LineIndex) {
        let p = temp(content);
        let src = Arc::new(source::open(&p).unwrap());
        let idx = LineIndex::new(src.len());
        let ctx = egui::Context::default();
        let h = crate::indexer::spawn_indexer(src.clone(), idx.clone(), Encoding::Utf8, ctx);
        h.join().unwrap();
        (src, idx)
    }

    #[test]
    fn distinct_counts_values() {
        let (src, idx) = open_indexed(b"a\nb\na\nc\na\n");
        let result = extract_distinct(&src, &idx, Encoding::Utf8, b',', 0, 0, None);
        assert_eq!(
            result.values,
            vec![("a".to_string(), 3), ("b".to_string(), 1), ("c".to_string(), 1)]
        );
        assert!(!result.truncated);
    }

    #[test]
    fn distinct_respects_data_start() {
        // 헤더(0행) 제외.
        let (src, idx) = open_indexed(b"name\na\nb\na\n");
        let result = extract_distinct(&src, &idx, Encoding::Utf8, b',', 0, 1, None);
        assert_eq!(result.values, vec![("a".to_string(), 2), ("b".to_string(), 1)]);
    }

    #[test]
    fn distinct_unquotes_values() {
        let (src, idx) = open_indexed(b"\"a,b\"\nc\n");
        let result = extract_distinct(&src, &idx, Encoding::Utf8, b',', 0, 0, None);
        assert_eq!(result.values, vec![("a,b".to_string(), 1), ("c".to_string(), 1)]);
    }

    #[test]
    fn filter_by_included_set() {
        // 원본: 0 apple, 1 banana, 2 cherry, 3 apple
        let (src, idx) = open_indexed(b"apple\nbanana\ncherry\napple\n");
        let mut included = HashSet::new();
        included.insert("apple".to_string());
        let f = ColumnFilter { contains: String::new(), included: Some(included) };
        let matched = apply_filters(&src, &idx, Encoding::Utf8, b',', &[(0, f)], 0, None);
        assert_eq!(matched, vec![0, 3]);
    }

    #[test]
    fn filter_by_contains_case_insensitive() {
        let (src, idx) = open_indexed(b"Apple\nbanana\nPineapple\n");
        let f = ColumnFilter { contains: "APP".to_string(), included: None };
        let matched = apply_filters(&src, &idx, Encoding::Utf8, b',', &[(0, f)], 0, None);
        assert_eq!(matched, vec![0, 2]);
    }

    #[test]
    fn filter_combines_multiple_columns_with_and() {
        // 원본: 0 "A,10", 1 "A,20", 2 "B,10"
        let (src, idx) = open_indexed(b"A,10\nA,20\nB,10\n");
        let mut included = HashSet::new();
        included.insert("A".to_string());
        let f0 = ColumnFilter { contains: String::new(), included: Some(included) };
        let f1 = ColumnFilter { contains: "10".to_string(), included: None };
        let matched = apply_filters(&src, &idx, Encoding::Utf8, b',', &[(0, f0), (1, f1)], 0, None);
        assert_eq!(matched, vec![0]);
    }

    #[test]
    fn filter_empty_conditions_match_everything() {
        let (src, idx) = open_indexed(b"a\nb\nc\n");
        let matched = apply_filters(&src, &idx, Encoding::Utf8, b',', &[], 0, None);
        assert_eq!(matched, vec![0, 1, 2]);
    }

    // ---- 편집 버퍼(인메모리) 경로 — mmap 경로와 같은 동작이어야 한다 ----

    #[test]
    fn apply_filters_lines_matches_mmap_path() {
        let lines: Vec<String> = ["apple", "banana", "cherry", "apple"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut included = HashSet::new();
        included.insert("apple".to_string());
        let f = ColumnFilter { contains: String::new(), included: Some(included) };
        let matched = apply_filters_lines(&lines, &[(0, f)], b',', 0);
        assert_eq!(matched, vec![0, 3]);
    }

    #[test]
    fn apply_filters_lines_contains_case_insensitive() {
        let lines: Vec<String> =
            ["Apple", "banana", "Pineapple"].iter().map(|s| s.to_string()).collect();
        let f = ColumnFilter { contains: "APP".to_string(), included: None };
        let matched = apply_filters_lines(&lines, &[(0, f)], b',', 0);
        assert_eq!(matched, vec![0, 2]);
    }

    #[test]
    fn apply_filters_lines_respects_data_start() {
        let lines: Vec<String> =
            ["name", "a", "b", "a"].iter().map(|s| s.to_string()).collect();
        let mut included = HashSet::new();
        included.insert("a".to_string());
        let f = ColumnFilter { contains: String::new(), included: Some(included) };
        let matched = apply_filters_lines(&lines, &[(0, f)], b',', 1);
        assert_eq!(matched, vec![1, 3]);
    }

    #[test]
    fn extract_distinct_lines_matches_mmap_path() {
        let lines: Vec<String> =
            ["a", "b", "a", "c", "a"].iter().map(|s| s.to_string()).collect();
        let result = extract_distinct_lines(&lines, b',', 0, 0);
        assert_eq!(
            result.values,
            vec![("a".to_string(), 3), ("b".to_string(), 1), ("c".to_string(), 1)]
        );
        assert_eq!(result.count, DistinctCount::Exact(3));
    }

    #[test]
    fn distinct_truncates_when_too_many_values() {
        // MAX_DISTINCT_VALUES보다 많은 고유값.
        let mut content = String::new();
        for i in 0..(MAX_DISTINCT_VALUES + 10) {
            content.push_str(&format!("v{i}\n"));
        }
        let (src, idx) = open_indexed(content.as_bytes());
        let result = extract_distinct(&src, &idx, Encoding::Utf8, b',', 0, 0, None);
        assert_eq!(result.values.len(), MAX_DISTINCT_VALUES);
        assert!(result.truncated);
    }

    #[test]
    fn distinct_count_is_exact_under_cap() {
        // EXACT_COUNT_CAP(5,000)보다 훨씬 적은 고유값 — 정확한 개수가 나와야 한다.
        // (MAX_DISTINCT_VALUES=1,000이라 체크박스 목록은 잘려도, 총 개수는
        // 정확해야 한다 — 목록 표시 상한과 카운트 정확도 상한은 별개다.)
        let mut content = String::new();
        for i in 0..1500 {
            content.push_str(&format!("v{i}\n"));
        }
        let (src, idx) = open_indexed(content.as_bytes());
        let result = extract_distinct(&src, &idx, Encoding::Utf8, b',', 0, 0, None);
        assert_eq!(result.count, DistinctCount::Exact(1500));
        assert!(result.truncated, "목록 표시는 1,000개로 잘려야 한다");
    }

    #[test]
    fn distinct_count_is_approx_over_cap() {
        // EXACT_COUNT_CAP(5,000)을 넘는 고유값 — 근사치로 전환돼야 하고, 그
        // 근사치가 터무니없이 틀리면 안 된다(HyperLogLog 표준오차 ~0.8%지만
        // 넉넉하게 20% 이내로만 확인해 흔들림에 안전하게).
        let true_count = 8000usize;
        let mut content = String::new();
        for i in 0..true_count {
            content.push_str(&format!("v{i}\n"));
        }
        let (src, idx) = open_indexed(content.as_bytes());
        let result = extract_distinct(&src, &idx, Encoding::Utf8, b',', 0, 0, None);
        let DistinctCount::Approx(estimate) = result.count else {
            panic!("상한을 넘겼으니 근사치여야 한다: {:?}", result.count);
        };
        let diff = (estimate as i64 - true_count as i64).unsigned_abs();
        assert!(
            (diff as f64 / true_count as f64) < 0.2,
            "근사치 {estimate}가 실제 {true_count}와 너무 다르다"
        );
    }

    #[test]
    fn hyperloglog_estimates_within_tolerance() {
        // HLL 자체 단위 테스트 — 10,000개의 서로 다른 문자열을 넣고 추정치가
        // 표준오차(~0.8%)의 몇 배 안쪽인지 확인한다.
        let mut hll = HyperLogLog::new();
        let n = 10_000;
        for i in 0..n {
            hll.insert(&format!("item-{i}"));
        }
        let estimate = hll.estimate();
        let diff = (estimate as i64 - n as i64).unsigned_abs();
        assert!(
            (diff as f64 / n as f64) < 0.1,
            "HLL 추정치 {estimate}가 실제 {n}와 10% 넘게 차이난다"
        );
    }

    #[test]
    fn hyperloglog_merge_matches_single_pass() {
        // 두 개로 나눠 넣고 merge한 결과가, 한 번에 다 넣은 것과 비슷한
        // 추정치를 내는지(reduce의 청크 병합 경로가 실제로 쓰는 연산).
        let mut a = HyperLogLog::new();
        let mut b = HyperLogLog::new();
        let mut whole = HyperLogLog::new();
        for i in 0..3000 {
            let s = format!("x-{i}");
            a.insert(&s);
            whole.insert(&s);
        }
        for i in 3000..6000 {
            let s = format!("x-{i}");
            b.insert(&s);
            whole.insert(&s);
        }
        a.merge(&b);
        let merged_estimate = a.estimate();
        let whole_estimate = whole.estimate();
        let diff = (merged_estimate as i64 - whole_estimate as i64).unsigned_abs();
        assert!(
            diff < whole_estimate / 20,
            "병합 추정치 {merged_estimate}가 단일 스캔 추정치 {whole_estimate}와 크게 다르다"
        );
    }
}
