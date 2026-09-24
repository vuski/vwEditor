use crate::index::LineIndex;
use crate::parse::{self, Encoding};
use crate::source::Source;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKind {
    Text,
    Number,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// 다중 컬럼 정렬의 한 기준: 어떤 컬럼을 어떤 종류/방향으로 정렬할지.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub col: usize,
    pub kind: SortKind,
    pub dir: SortDir,
    /// 문자 정렬에서 대소문자 무시(case-insensitive) 여부. true면 ASCII 대문자를
    /// 소문자로 접어 비교(A와 a를 같게). 숫자 정렬에선 무시된다.
    pub ci: bool,
}

/// 다중 정렬 최대 기준 수. 키를 고정 배열 `[u64; MAX_KEYS]`로 담아 캐시 친화적
/// 정수 비교를 한다. 4개면 대부분의 실사용을 커버한다.
pub const MAX_KEYS: usize = 4;

/// 문자 키에 인라인으로 담는 접두부 바이트 수. 앞 8바이트를 u64 하나로 pack해
/// 정수 비교 한 번으로 대소를 가른다. 8바이트가 완전히 같은 경우에만(그리고
/// 필드가 8바이트를 넘을 때만) 원본 바이트로 tie-break 한다.
///
/// u128(16B) 대신 u64(8B)를 쓰는 이유: 정렬 배열 크기가 절반이 되어(수억 행
/// 규모에서 GB 단위 차이) 캐시 효율이 크게 올라 정렬 자체가 빨라진다. 8바이트
/// prefix로도 대부분의 실데이터 컬럼은 앞부분에서 대소가 갈린다.
const PREFIX_LEN: usize = 8;

/// 정렬가능한 고정 크기 정수 키. 문자/숫자 모두 u64 하나로 표현해 정렬 배열을
/// 작게 유지한다(파싱/할당 없음).
///
/// - **문자**: 필드 앞 8바이트를 big-endian으로 pack. big-endian이라 정수 대소 =
///   바이트 사전식 순서(앞 글자가 상위 비트). 짧은 필드는 뒤를 0으로 패딩 →
///   "짧은 게 먼저"(ab < abc)가 자동 성립.
/// - **숫자**: f64를 "정렬가능 u64"로 변환(부호비트 트릭). 비수치/빈값은 최댓값
///   (맨 뒤)인 NUM_INVALID.
type Key = u64;

/// 숫자 정렬에서 비수치/빈값/컬럼 없음을 나타내는 키. 항상 최댓값이라 오름차순에서
/// 맨 뒤로 간다. (유효 숫자의 정렬키는 f64_sortable 특성상 절대 u64::MAX가 되지
/// 않으므로 — INFINITY도 0xFFF0...< MAX — 충돌하지 않는다.)
const NUM_INVALID: Key = Key::MAX;

/// 문자 필드 바이트 → big-endian pack u64 키(앞 8바이트).
/// `ci`(case-insensitive)면 ASCII 대문자(A-Z)를 소문자로 접어 대소문자를 같게
/// 취급한다. 비-ASCII(한글 등 UTF-8 멀티바이트)는 그대로 두므로, ASCII 대소문자
/// 구분만 무시하는 실용적 동작(대부분의 실데이터 요구를 충족).
pub(crate) fn text_key(field: &[u8], ci: bool) -> Key {
    let mut buf = [0u8; PREFIX_LEN];
    let n = field.len().min(PREFIX_LEN);
    buf[..n].copy_from_slice(&field[..n]);
    if ci {
        for b in buf[..n].iter_mut() {
            b.make_ascii_lowercase();
        }
    }
    Key::from_be_bytes(buf)
}

/// f64 → 정렬가능한 u64 비트패턴. IEEE754는 부호비트가 켜지면(음수) 비트열이
/// 역순이 되므로, 양수는 최상위 비트를 세우고 음수는 전 비트를 뒤집으면
/// 부호 없는 정수 비교가 원래 f64 대소와 일치한다(NaN은 total order 기준).
fn f64_sortable(v: f64) -> u64 {
    let bits = v.to_bits();
    if bits & (1 << 63) != 0 {
        // 음수(부호비트 1): 전 비트 반전.
        !bits
    } else {
        // 양수/0: 부호비트만 세움.
        bits | (1 << 63)
    }
}

/// 숫자 필드 바이트 → 키. 파싱 성공하면 정렬가능 u64, 실패(비수치/빈값)면
/// NUM_INVALID(최댓값, 맨 뒤).
pub(crate) fn number_key(field: &[u8]) -> Key {
    // 앞뒤 공백 제거 후 f64 파싱. field는 raw 바이트라 str로 본 뒤 파싱.
    let s = match std::str::from_utf8(field) {
        Ok(s) => s.trim(),
        Err(_) => return NUM_INVALID,
    };
    match s.parse::<f64>() {
        Ok(v) => f64_sortable(v),
        Err(_) => NUM_INVALID,
    }
}

/// 백그라운드에서 도는 정렬 작업의 공유 상태. UI는 매 프레임 이 상태를 폴링해
/// 진행률 바를 그리고, `result`가 채워지면 permutation을 가져간다.
struct SortShared {
    /// 키 추출이 처리한 데이터 행 수(진행률 분자).
    rows_done: AtomicU64,
    /// 전체 데이터 행 수(진행률 분모).
    total_rows: u64,
    /// 정렬까지 끝나 permutation이 준비되면 Some.
    result: Mutex<Option<Vec<u32>>>,
    /// 작업 완료(성공/취소 무관) 플래그.
    finished: AtomicBool,
}

/// 백그라운드 정렬 작업 핸들. UI가 소유하며 진행률 폴링 + 결과 수거에 쓴다.
pub struct SortJob {
    shared: Arc<SortShared>,
    handle: Option<std::thread::JoinHandle<()>>,
    /// 완료 후 SortState 구성용. 다중 정렬이면 1차 기준(화살표 표시용).
    pub col: usize,
    pub kind: SortKind,
    pub dir: SortDir,
    /// 다중 정렬 기준(2개 이상이면 다중). 단일이면 비어 있다.
    pub specs: Vec<SortSpec>,
}

impl SortJob {
    /// 진행률 0.0~1.0. total이 0이면 1.0.
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

    /// 완료됐으면 permutation을 꺼낸다(한 번만). 미완료면 None.
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

/// 정렬을 백그라운드 스레드에서 시작한다. UI를 막지 않으며 진행률을 폴링할 수
/// 있다. ctx는 진행 중 화면 갱신을 요청하는 데 쓴다.
#[allow(clippy::too_many_arguments)]
pub fn spawn_sort(
    source: Arc<Source>,
    index: LineIndex,
    enc: Encoding,
    delim: u8,
    col: usize,
    data_start: usize,
    kind: SortKind,
    dir: SortDir,
    ci: bool,
    ctx: egui::Context,
) -> SortJob {
    let total_rows = (index.line_count().saturating_sub(data_start)) as u64;
    let shared = Arc::new(SortShared {
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
        let perm = extract_and_sort(
            &source,
            &index,
            enc,
            delim,
            col,
            data_start,
            kind,
            dir,
            ci,
            Some(&progress),
        );
        // 진행률을 100%로 마무리.
        shared_bg
            .rows_done
            .store(shared_bg.total_rows, Ordering::Relaxed);
        *shared_bg.result.lock().unwrap() = Some(perm);
        shared_bg.finished.store(true, Ordering::Relaxed);
        ctx.request_repaint();
    });

    SortJob {
        shared,
        handle: Some(handle),
        col,
        kind,
        dir,
        specs: Vec::new(),
    }
}

/// 다중 컬럼 정렬을 백그라운드에서 시작한다. specs는 1차→N차 우선순위.
/// SortJob의 col/kind/dir은 1차 기준(헤더 화살표 표시용).
pub fn spawn_multi_sort(
    source: Arc<Source>,
    index: LineIndex,
    enc: Encoding,
    delim: u8,
    specs: Vec<SortSpec>,
    data_start: usize,
    ctx: egui::Context,
) -> SortJob {
    let total_rows = (index.line_count().saturating_sub(data_start)) as u64;
    let shared = Arc::new(SortShared {
        rows_done: AtomicU64::new(0),
        total_rows,
        result: Mutex::new(None),
        finished: AtomicBool::new(false),
    });
    // 1차 기준(비었으면 기본값 — 호출측이 빈 specs로 부르지 않도록 보장).
    let first = specs.first().copied().unwrap_or(SortSpec {
        col: 0,
        kind: SortKind::Text,
        dir: SortDir::Asc,
        ci: true,
    });

    let shared_bg = shared.clone();
    let specs_bg = specs.clone();
    let handle = std::thread::spawn(move || {
        let progress = {
            let shared = shared_bg.clone();
            let ctx = ctx.clone();
            move |n: usize| {
                shared.rows_done.fetch_add(n as u64, Ordering::Relaxed);
                ctx.request_repaint();
            }
        };
        let perm = extract_and_multi_sort(
            &source,
            &index,
            enc,
            delim,
            &specs_bg,
            data_start,
            Some(&progress),
        );
        shared_bg
            .rows_done
            .store(shared_bg.total_rows, Ordering::Relaxed);
        *shared_bg.result.lock().unwrap() = Some(perm);
        shared_bg.finished.store(true, Ordering::Relaxed);
        ctx.request_repaint();
    });

    SortJob {
        shared,
        handle: Some(handle),
        col: first.col,
        kind: first.kind,
        dir: first.dir,
        specs,
    }
}

/// 선택 컬럼 값을 추출해 정렬하고, **데이터 행 번호의 permutation**을 반환한다.
/// 반환된 `Vec<u32>`의 i번째 원소는 "정렬 순서 i번째로 보여줄 원본 데이터 행의
/// 절대 논리 행번호"다.
///
/// # 성능
/// 행마다 String/Vec 할당을 하지 않는다. mmap raw 바이트에서 `field_slice`로
/// 해당 컬럼 슬라이스만 잡아 **고정 크기 u128 키를 한 번만** 만들고,
/// `par_sort_unstable_by_key`로 순수 정수 비교 정렬한다.
///
/// - `data_start`: 데이터 시작 논리 행번호(has_header면 1, 아니면 0).
/// - 헤더 행(있으면 0행)은 정렬 대상에서 제외되며 permutation에 포함되지 않는다.
/// - 비수치(숫자 정렬)/빈값/컬럼 없는 행은 방향과 무관하게 맨 뒤로 간다.
/// - 안정성: 키가 같으면 원본 행번호 순서를 유지(tie-break).
/// - `progress`: 키 추출 진행을 알리는 콜백(처리한 행 수 누적). None이면 무시.
#[allow(clippy::too_many_arguments)]
pub fn extract_and_sort(
    source: &Arc<Source>,
    index: &LineIndex,
    enc: Encoding,
    delim: u8,
    col: usize,
    data_start: usize,
    kind: SortKind,
    dir: SortDir,
    ci: bool,
    progress: Option<&(dyn Fn(usize) + Sync)>,
) -> Vec<u32> {
    let total = index.line_count();
    if total <= data_start {
        return Vec::new();
    }
    let data_rows = total - data_start;

    // offset 배열을 한 번만 snapshot(락 없는 Arc 뷰). 이후 워커들은 이 슬라이스를
    // 직접 인덱싱해 RwLock을 행마다 잡지 않는다(3억 행 규모에서 락이 병목).
    let (offsets, total_bytes) = index.snapshot();
    let offsets: &[u64] = &offsets;
    let bytes = source.as_bytes();

    // 각 데이터 행에서 선택 컬럼의 고정 크기 정수 키를 추출.
    // 키 쌍은 (key, truncated, row): truncated=true면 필드가 PREFIX_LEN(8B)을
    // 넘어 키에 다 안 담겼다는 뜻이라, 키 동률 시 원본 tie-break가 필요하다.
    // 8B 이하면 키 동률 = 완전 동일이므로 tie-break를 건너뛴다.
    //
    // 결과 배열을 미리 할당하고 큰 청크로 나눠(par_chunks_mut) 각 워커가 자기
    // 청크를 **순차** 순회하게 한다 → mmap 순차 접근(prefetch 친화적)으로
    // 랜덤 접근 대비 캐시 효율이 크게 오른다.
    let mut keyed: Vec<(Key, bool, u32)> = vec![(0, false, 0); data_rows];
    const CHUNK: usize = 64 * 1024; // 워커당 순차 처리 단위
    let done_counter = std::sync::atomic::AtomicUsize::new(0);
    keyed
        .par_chunks_mut(CHUNK)
        .enumerate()
        .for_each(|(chunk_idx, chunk)| {
            let base = chunk_idx * CHUNK;
            for (j, slot) in chunk.iter_mut().enumerate() {
                let i = base + j;
                let logical = data_start + i;
                let (key, truncated) =
                    extract_key_fast(bytes, offsets, total_bytes, enc, delim, col, kind, ci, logical);
                *slot = (key, truncated, logical as u32);
            }
            if let Some(p) = progress {
                // 청크 하나 끝날 때마다 처리 행 수 보고(콜백 빈도 낮춤).
                let d = done_counter.fetch_add(chunk.len(), Ordering::Relaxed) + chunk.len();
                let _ = d;
                p(chunk.len());
            }
        });

    // 정수 키 비교로 정렬. 키가 같을 때:
    // - 문자 정렬 & 양쪽 모두 truncated: 앞 8바이트(PREFIX_LEN)만 같을 뿐 뒤가 다를 수
    //   있으므로 원본 전체로 tie-break. 한쪽이라도 안 잘렸으면 키가 전체를
    //   담았으므로 tie-break 불필요.
    // - 그 외: 행번호로 안정화.
    keyed.par_sort_unstable_by(|a, b| {
        match a.0.cmp(&b.0) {
            std::cmp::Ordering::Equal => {
                if kind == SortKind::Text && a.1 && b.1 {
                    // 앞 8바이트만 같음(양쪽 truncated) → 원본 전체로 tie-break.
                    // 락 없는 offsets 슬라이스 + mmap 바이트로 직접 비교.
                    let mut fa = full_field_fast(bytes, offsets, total_bytes, enc, delim, col, a.2 as usize)
                        .unwrap_or_default();
                    let mut fb = full_field_fast(bytes, offsets, total_bytes, enc, delim, col, b.2 as usize)
                        .unwrap_or_default();
                    // ci면 tie-break 비교도 소문자로 접어야 앞 키와 정책이 일관된다.
                    if ci {
                        fa.make_ascii_lowercase();
                        fb.make_ascii_lowercase();
                    }
                    fa.cmp(&fb).then_with(|| a.2.cmp(&b.2))
                } else {
                    a.2.cmp(&b.2)
                }
            }
            ord => ord,
        }
    });

    if dir == SortDir::Desc {
        // 내림차순: 유효 키만 뒤집고, 무효 키(숫자 비수치 = NUM_INVALID)는 끝에
        // 유지. 문자 정렬엔 무효 키가 없어 전체 reverse와 동일해진다.
        let invalid_from =
            keyed.partition_point(|(k, _, _)| *k != NUM_INVALID || kind != SortKind::Number);
        keyed[..invalid_from].reverse();
    }

    keyed.into_iter().map(|(_, _, idx)| idx).collect()
}

/// 한 기준(SortSpec)에 대해 한 행의 컬럼 값 → 방향 반영 u64 키.
/// 다중 정렬용: 방향을 키에 인코딩해 `[u64; N]` 배열의 단일 정수 비교로 오름/내림
/// 혼합을 표현한다. 내림(Desc)은 유효 키를 비트 반전(`!key`)하되, 숫자 비수치
/// (NUM_INVALID=u64::MAX)는 반전하지 않아 방향 무관하게 맨 뒤로 남긴다.
///
/// 다중 정렬은 tie-break 원본 재접근 없이 앞 8바이트 prefix로만 비교한다(설계상
/// 단순화). truncated는 여기서 쓰지 않는다.
pub(crate) fn col_key(field: Option<&[u8]>, spec: SortSpec) -> u64 {
    let raw = match spec.kind {
        SortKind::Text => match field {
            Some(f) => text_key(f, spec.ci),
            None => 0, // 컬럼 없음/빈값 = 사전순 맨 앞
        },
        SortKind::Number => match field {
            Some(f) => number_key(f), // 비수치면 NUM_INVALID
            None => NUM_INVALID,
        },
    };
    match spec.dir {
        SortDir::Asc => raw,
        SortDir::Desc => {
            // 숫자 비수치는 항상 맨 뒤 → 반전하지 않는다.
            if spec.kind == SortKind::Number && raw == NUM_INVALID {
                NUM_INVALID
            } else {
                !raw
            }
        }
    }
}

/// 한 행에서 specs 순서대로 키 배열 `[u64; MAX_KEYS]`를 만든다(미사용 슬롯 0).
fn multi_key_for_row(
    bytes: &[u8],
    offsets: &[u64],
    total_bytes: u64,
    enc: Encoding,
    delim: u8,
    specs: &[SortSpec],
    logical: usize,
) -> [u64; MAX_KEYS] {
    let mut keys = [0u64; MAX_KEYS];
    let Some((s, e)) = LineIndex::range_in(offsets, total_bytes, logical) else {
        // 행 범위를 못 얻으면 각 기준의 "빈 값" 키로.
        for (k, spec) in keys.iter_mut().zip(specs.iter()) {
            *k = col_key(None, *spec);
        }
        return keys;
    };
    let raw = &bytes[s as usize..e as usize];

    // UTF-16은 field_slice(단일바이트 전용)가 부정확하므로 디코딩 폴백.
    match enc {
        Encoding::Utf8 | Encoding::Cp949 => {
            let line = trim_newline(raw);
            for (slot, spec) in keys.iter_mut().zip(specs.iter()) {
                let field = parse::field_slice(line, delim, spec.col);
                *slot = col_key(field, *spec);
            }
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let text = parse::decode_line(raw, enc);
            let fields = parse::split_fields(text.trim_end_matches(['\r', '\n']), delim);
            for (slot, spec) in keys.iter_mut().zip(specs.iter()) {
                let field = fields.get(spec.col).map(|f| f.as_bytes());
                *slot = col_key(field, *spec);
            }
        }
    }
    keys
}

/// 다중 컬럼 정렬. specs를 앞에서부터(1차→N차) 우선순위로 적용해 permutation을
/// 만든다. 단일 `extract_and_sort`의 다중 버전 — 같은 락 없는 snapshot + 순차
/// 순회 인프라를 쓴다. specs가 비었으면 빈 permutation.
pub fn extract_and_multi_sort(
    source: &Arc<Source>,
    index: &LineIndex,
    enc: Encoding,
    delim: u8,
    specs: &[SortSpec],
    data_start: usize,
    progress: Option<&(dyn Fn(usize) + Sync)>,
) -> Vec<u32> {
    let total = index.line_count();
    if total <= data_start || specs.is_empty() {
        return Vec::new();
    }
    let data_rows = total - data_start;

    let (offsets, total_bytes) = index.snapshot();
    let offsets: &[u64] = &offsets;
    let bytes = source.as_bytes();

    // 키 배열 + 원본 행번호. par_chunks_mut로 각 워커가 자기 구간 순차 순회.
    let mut keyed: Vec<([u64; MAX_KEYS], u32)> = vec![([0u64; MAX_KEYS], 0); data_rows];
    const CHUNK: usize = 64 * 1024;
    keyed.par_chunks_mut(CHUNK).enumerate().for_each(|(ci, chunk)| {
        let base = ci * CHUNK;
        for (j, slot) in chunk.iter_mut().enumerate() {
            let logical = data_start + base + j;
            let keys = multi_key_for_row(bytes, offsets, total_bytes, enc, delim, specs, logical);
            *slot = (keys, logical as u32);
        }
        if let Some(p) = progress {
            p(chunk.len());
        }
    });

    // 키 배열 사전식 비교(방향은 키에 이미 인코딩됨) → 동률이면 행번호로 안정화.
    keyed.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    keyed.into_iter().map(|(_, idx)| idx).collect()
}

/// `extract_and_multi_sort`의 부분집합 버전. 파일 전체(`data_start..total`)
/// 대신 `rows`(논리 행번호 목록, 임의 순서)만 정렬 대상으로 삼는다 — 컬럼
/// 필터가 걸린 뒤 "그 결과 안에서만 정렬"할 때 쓴다(엑셀에서 필터+정렬을
/// 같이 쓰는 것과 같은 동작). 키 인코딩·정렬 로직은 `extract_and_multi_sort`와
/// 완전히 같고, 순회 범위(`data_start..total` vs 임의의 `rows` 목록)만 다르다
/// — 그래서 함수를 합치지 않고 따로 둔다(제어 흐름을 억지로 통일하면 오히려
/// 어느 쪽 경로인지 읽기 어려워진다).
pub fn extract_and_multi_sort_subset(
    source: &Arc<Source>,
    index: &LineIndex,
    enc: Encoding,
    delim: u8,
    specs: &[SortSpec],
    rows: &[u32],
    progress: Option<&(dyn Fn(usize) + Sync)>,
) -> Vec<u32> {
    if rows.is_empty() || specs.is_empty() {
        return Vec::new();
    }
    let (offsets, total_bytes) = index.snapshot();
    let offsets: &[u64] = &offsets;
    let bytes = source.as_bytes();

    let mut keyed: Vec<([u64; MAX_KEYS], u32)> = vec![([0u64; MAX_KEYS], 0); rows.len()];
    const CHUNK: usize = 64 * 1024;
    keyed.par_chunks_mut(CHUNK).enumerate().for_each(|(ci, chunk)| {
        let base = ci * CHUNK;
        for (j, slot) in chunk.iter_mut().enumerate() {
            let logical = rows[base + j] as usize;
            let keys = multi_key_for_row(bytes, offsets, total_bytes, enc, delim, specs, logical);
            *slot = (keys, rows[base + j]);
        }
        if let Some(p) = progress {
            p(chunk.len());
        }
    });

    keyed.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    keyed.into_iter().map(|(_, idx)| idx).collect()
}

/// 한 행의 선택 컬럼에서 정렬 키를 추출한다(락 없는 hot path 버전).
/// offsets 슬라이스와 전체 mmap 바이트를 직접 받아 RwLock을 잡지 않는다.
/// 반환: (정렬 키, truncated). truncated=true면 필드가 PREFIX_LEN(8B)을 넘어
/// 키에 앞 8바이트만 담겼다는 뜻(문자 tie-break 필요 판단용). 숫자 키는 항상 false.
#[allow(clippy::too_many_arguments)]
fn extract_key_fast(
    bytes: &[u8],
    offsets: &[u64],
    total_bytes: u64,
    enc: Encoding,
    delim: u8,
    col: usize,
    kind: SortKind,
    ci: bool,
    logical: usize,
) -> (Key, bool) {
    let Some((s, e)) = LineIndex::range_in(offsets, total_bytes, logical) else {
        return (empty_key(kind), false);
    };
    let raw = &bytes[s as usize..e as usize];

    match enc {
        Encoding::Utf8 | Encoding::Cp949 => {
            let line = trim_newline(raw);
            match parse::field_slice(line, delim, col) {
                Some(field) => make_key(field, kind, ci),
                None => (empty_key(kind), false),
            }
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let text = parse::decode_line(raw, enc);
            let fields = parse::split_fields(text.trim_end_matches(['\r', '\n']), delim);
            match fields.get(col) {
                Some(f) => make_key(f.as_bytes(), kind, ci),
                None => (empty_key(kind), false),
            }
        }
    }
}

/// tie-break용(락 없는 버전): 한 행의 선택 컬럼 전체 바이트를 복사해 반환한다.
/// 문자 키 앞 8바이트 동률일 때만 호출되므로 드물다.
#[allow(clippy::too_many_arguments)]
fn full_field_fast(
    bytes: &[u8],
    offsets: &[u64],
    total_bytes: u64,
    enc: Encoding,
    delim: u8,
    col: usize,
    logical: usize,
) -> Option<Vec<u8>> {
    let (s, e) = LineIndex::range_in(offsets, total_bytes, logical)?;
    let raw = &bytes[s as usize..e as usize];
    match enc {
        Encoding::Utf8 | Encoding::Cp949 => {
            let line = trim_newline(raw);
            parse::field_slice(line, delim, col).map(|f| f.to_vec())
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let text = parse::decode_line(raw, enc);
            let fields = parse::split_fields(text.trim_end_matches(['\r', '\n']), delim);
            fields.get(col).map(|f| f.as_bytes().to_vec())
        }
    }
}

/// 컬럼이 없거나 빈 경우의 키. 숫자면 무효(맨 뒤), 문자면 빈 문자열 키(사전 맨 앞).
fn empty_key(kind: SortKind) -> Key {
    match kind {
        SortKind::Number => NUM_INVALID,
        SortKind::Text => 0, // 빈 필드는 사전순 맨 앞(모두 0 패딩)
    }
}

/// 반환: (키, truncated). 문자 키는 필드가 PREFIX_LEN(8바이트) 초과면 truncated=true.
/// 숫자 키는 값 전체를 담으므로 항상 false. `ci`는 문자 정렬 대소문자 무시.
fn make_key(field: &[u8], kind: SortKind, ci: bool) -> (Key, bool) {
    match kind {
        SortKind::Text => (text_key(field, ci), field.len() > PREFIX_LEN),
        SortKind::Number => (number_key(field), false),
    }
}

/// 인메모리 줄 배열을 다중 기준으로 정렬해 데이터 행(data_start..)의 permutation을
/// 반환한다. permutation[i] = 정렬 순서 i번째로 올 원본 논리 행번호.
/// mmap 경로(extract_and_multi_sort)와 동일한 키 인코딩을 쓴다.
pub fn sort_lines(lines: &[String], specs: &[SortSpec], delim: u8, data_start: usize) -> Vec<u32> {
    if lines.len() <= data_start || specs.is_empty() {
        return Vec::new();
    }
    let data_rows = lines.len() - data_start;
    let mut keyed: Vec<([u64; MAX_KEYS], u32)> = (0..data_rows)
        .into_par_iter()
        .map(|i| {
            let logical = data_start + i;
            let line = lines[logical].as_bytes();
            let mut keys = [0u64; MAX_KEYS];
            for (slot, spec) in keys.iter_mut().zip(specs.iter()) {
                let field = parse::field_slice(line, delim, spec.col);
                *slot = col_key(field, *spec);
            }
            (keys, logical as u32)
        })
        .collect();
    keyed.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    keyed.into_iter().map(|(_, idx)| idx).collect()
}

/// `sort_lines`의 부분집합 버전. 전체(`data_start..`) 대신 `rows`(논리
/// 행번호 목록, 임의 순서)만 정렬 대상으로 삼는다 — 편집 모드에서 필터가
/// 걸린 뒤 그 결과만(파일은 그대로 두고 **화면 표시 순서만**) 정렬할 때 쓴다.
/// `extract_and_multi_sort_subset`(mmap 경로)의 인메모리 버전.
pub fn sort_lines_subset(lines: &[String], specs: &[SortSpec], delim: u8, rows: &[u32]) -> Vec<u32> {
    if rows.is_empty() || specs.is_empty() {
        return Vec::new();
    }
    let mut keyed: Vec<([u64; MAX_KEYS], u32)> = rows
        .par_iter()
        .map(|&logical| {
            let line = lines[logical as usize].as_bytes();
            let mut keys = [0u64; MAX_KEYS];
            for (slot, spec) in keys.iter_mut().zip(specs.iter()) {
                let field = parse::field_slice(line, delim, spec.col);
                *slot = col_key(field, *spec);
            }
            (keys, logical)
        })
        .collect();
    keyed.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    keyed.into_iter().map(|(_, idx)| idx).collect()
}

/// 뒤쪽 CR/LF만 제거한 슬라이스.
fn trim_newline(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b'\n' || b[end - 1] == b'\r') {
        end -= 1;
    }
    &b[..end]
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
        p.push(format!("tv_sort_{}_{}.csv", std::process::id(), id));
        std::fs::File::create(&p).unwrap().write_all(content).unwrap();
        p
    }

    /// 파일을 열어 인덱싱 완료까지 기다린 (Source, LineIndex)를 만든다.
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
    fn text_sort_ascending() {
        // 컬럼 0을 문자 오름차순: banana, apple, cherry → apple, banana, cherry
        // 원본 행번호: 0 banana, 1 apple, 2 cherry → permutation [1,0,2]
        let (src, idx) = open_indexed(b"banana\napple\ncherry\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, false, None);
        assert_eq!(perm, vec![1, 0, 2]);
    }

    #[test]
    fn text_sort_descending() {
        let (src, idx) = open_indexed(b"banana\napple\ncherry\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Desc, false, None);
        // cherry, banana, apple → 원본 [2,0,1]
        assert_eq!(perm, vec![2, 0, 1]);
    }

    #[test]
    fn number_sort_beats_lexicographic() {
        // 문자면 "10" < "2" < "9" 지만, 숫자면 2 < 9 < 10 이어야 한다.
        // 원본: 0 "10", 1 "2", 2 "9" → 숫자 오름 permutation [1,2,0]
        let (src, idx) = open_indexed(b"10\n2\n9\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Number, SortDir::Asc, false, None);
        assert_eq!(perm, vec![1, 2, 0]);
    }

    #[test]
    fn number_sort_descending() {
        let (src, idx) = open_indexed(b"10\n2\n9\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Number, SortDir::Desc, false, None);
        // 10, 9, 2 → 원본 [0,2,1]
        assert_eq!(perm, vec![0, 2, 1]);
    }

    #[test]
    fn non_numeric_goes_last_ascending() {
        // 숫자 정렬에서 비수치("abc")는 맨 뒤. 원본: 0 "5", 1 "abc", 2 "1"
        // 오름: 1, 5, [abc] → [2,0,1]
        let (src, idx) = open_indexed(b"5\nabc\n1\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Number, SortDir::Asc, false, None);
        assert_eq!(perm, vec![2, 0, 1]);
    }

    #[test]
    fn non_numeric_goes_last_descending_too() {
        // 내림차순에서도 비수치는 맨 뒤(방향 무관하게 뒤).
        let (src, idx) = open_indexed(b"5\nabc\n1\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Number, SortDir::Desc, false, None);
        // 내림: 5, 1, [abc] → [0,2,1]
        assert_eq!(perm, vec![0, 2, 1]);
    }

    #[test]
    fn stable_on_equal_keys() {
        // 같은 키(둘 다 "x")면 원본 행번호 순서 유지(tie-break).
        // 원본: 0 "x", 1 "a", 2 "x" → 문자 오름: a(1), x(0), x(2) → [1,0,2]
        let (src, idx) = open_indexed(b"x\na\nx\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, false, None);
        assert_eq!(perm, vec![1, 0, 2]);
    }

    #[test]
    fn sorts_second_column() {
        // 2번째 컬럼(col=1) 숫자 오름. name,age 형태.
        // 원본(헤더 제외 가정 data_start=0): 0 "a,30", 1 "b,10", 2 "c,20"
        // age 오름: 10(1),20(2),30(0) → [1,2,0]
        let (src, idx) = open_indexed(b"a,30\nb,10\nc,20\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 1, 0, SortKind::Number, SortDir::Asc, false, None);
        assert_eq!(perm, vec![1, 2, 0]);
    }

    #[test]
    fn header_row_excluded() {
        // has_header면 data_start=1. 헤더(0행)는 permutation에서 제외.
        // 원본: 0 "name"(헤더), 1 "banana", 2 "apple"
        // 문자 오름(데이터만): apple(2), banana(1) → [2,1]
        let (src, idx) = open_indexed(b"name\nbanana\napple\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 1, SortKind::Text, SortDir::Asc, false, None);
        assert_eq!(perm, vec![2, 1]);
    }

    #[test]
    fn missing_column_treated_as_empty_or_none() {
        // col=1을 요구하지만 어떤 행은 컬럼이 1개뿐. 숫자 정렬에서 그 행은 뒤로.
        // 원본: 0 "5,3", 1 "9"(col1 없음), 2 "5,1"
        // col1 숫자 오름: 1(row2), 3(row0), [none](row1) → [2,0,1]
        let (src, idx) = open_indexed(b"5,3\n9\n5,1\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 1, 0, SortKind::Number, SortDir::Asc, false, None);
        assert_eq!(perm, vec![2, 0, 1]);
    }

    #[test]
    fn text_tie_break_beyond_prefix() {
        // 앞 16바이트가 완전히 동일하고 17번째에서 갈리는 두 문자열.
        // 인라인 prefix(16B)만 보면 키가 같아 tie-break가 원본 전체를 비교해야
        // 정확한 순서가 나온다. prefix = "abcdefghijklmnop"(16자).
        // row0 = ...mnop_Z, row1 = ...mnop_A, row2 = ...mnop_M
        // 문자 오름: _A(1), _M(2), _Z(0) → [1,2,0]
        let (src, idx) = open_indexed(
            b"abcdefghijklmnop_Z\nabcdefghijklmnop_A\nabcdefghijklmnop_M\n",
        );
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, false, None);
        assert_eq!(perm, vec![1, 2, 0], "17번째 바이트 차이로 tie-break되어야 함");
    }

    #[test]
    fn text_sort_shorter_prefix_first() {
        // 사전순: "ab" < "abc" < "b". prefix pack + 0패딩이 이 순서를 내는지.
        let (src, idx) = open_indexed(b"b\nabc\nab\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, false, None);
        // ab(2), abc(1), b(0) → [2,1,0]
        assert_eq!(perm, vec![2, 1, 0]);
    }

    /// 실파일 정렬 성능 벤치. 환경변수 TV_BENCH_FILE로 tsv/csv 경로를 지정하면
    /// 맨 우측 컬럼 기준으로 숫자/문자 정렬 시간을 잰다(구분자는 확장자로 추정).
    ///   TV_BENCH_FILE=... cargo test --release bench_sort -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_sort() {
        use std::time::Instant;
        let path = match std::env::var("TV_BENCH_FILE") {
            Ok(p) => p,
            Err(_) => {
                eprintln!("TV_BENCH_FILE 미지정 — 스킵");
                return;
            }
        };
        let path = std::path::PathBuf::from(path);
        let delim = match path.extension().and_then(|e| e.to_str()) {
            Some("tsv") | Some("tab") => b'\t',
            _ => b',',
        };

        // 인덱싱(개행 스캔).
        let src = Arc::new(source::open(&path).unwrap());
        let idx = LineIndex::new(src.len());
        let ctx = egui::Context::default();
        let t_idx = Instant::now();
        crate::indexer::spawn_indexer(src.clone(), idx.clone(), Encoding::Utf8, ctx)
            .join()
            .unwrap();
        let idx_ms = t_idx.elapsed().as_secs_f64() * 1000.0;
        let rows = idx.line_count();
        let gb = src.len() as f64 / 1e9;

        // 맨 우측 컬럼 인덱스 = 첫 줄 필드 수 - 1.
        let first = {
            let (s, e) = idx.line_range(0).unwrap();
            let line = trim_newline(src.slice(s, e));
            let mut n = 1usize;
            for &b in line {
                if b == delim {
                    n += 1;
                }
            }
            n - 1
        };
        eprintln!(
            "file={} size={gb:.2}GB rows={rows} 맨우측col={first} (인덱싱 {idx_ms:.0}ms)",
            path.display()
        );

        // 숫자 정렬(맨 우측).
        let t = Instant::now();
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, delim, first, 0, SortKind::Number, SortDir::Asc, false, None);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "숫자 정렬: {ms:8.1} ms  ({:.1} M rows/s)  perm_len={}",
            rows as f64 / 1e6 / (ms / 1000.0),
            perm.len()
        );

        // 문자 정렬(맨 우측).
        let t = Instant::now();
        let perm2 = extract_and_sort(&src, &idx, Encoding::Utf8, delim, first, 0, SortKind::Text, SortDir::Asc, false, None);
        let ms2 = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "문자 정렬: {ms2:8.1} ms  ({:.1} M rows/s)  perm_len={}",
            rows as f64 / 1e6 / (ms2 / 1000.0),
            perm2.len()
        );
    }

    #[test]
    fn number_sort_negatives_and_decimals() {
        // 음수/소수 섞인 숫자 정렬(정렬가능 u64 변환 검증).
        // 원본: 0 "-3.5", 1 "2", 2 "-10", 3 "0.5"
        // 오름: -10(2), -3.5(0), 0.5(3), 2(1) → [2,0,3,1]
        let (src, idx) = open_indexed(b"-3.5\n2\n-10\n0.5\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Number, SortDir::Asc, false, None);
        assert_eq!(perm, vec![2, 0, 3, 1]);
    }

    // ---- field_slice 인용 처리 수정에 대한 회귀 테스트 ----
    //
    // `parse::field_slice`는 예전에 모든 `"`(필드 중간에 나온 것 포함)를
    // in_quotes 토글로 취급해, `5" pipe`처럼 필드 중간에 리터럴 `"`가 있으면
    // 그 뒤 구분자를 인용 안으로 착각해 다음 필드까지 삼켜버렸다. 그 결과
    // `sort.rs`가 뽑는 정렬 키도 어긋났다(잘못 병합된 필드 → 숫자 파싱 실패 →
    // NUM_INVALID로 맨 뒤). 수정 후에는 "필드 첫 바이트가 `\"`일 때만 인용
    // 시작"이므로 중간의 `"`는 리터럴로 남고 구분자가 정상적으로 필드를 가른다.
    // 이 섹션은 그 수정이 sort 경로에서 실제로 지켜지는지 고정한다 — `sort::`
    // 테스트를 되돌려도(예: field_slice가 다시 망가지면) 여기서 반드시 깨져야 한다.

    #[test]
    fn number_sort_column_after_unquoted_mid_field_quote() {
        // 컬럼 0(문자, 인용 없음)에 리터럴 `"`가 중간에 낀 세 행. 컬럼 1(숫자)
        // 기준으로 오름차순 정렬한다.
        //   row0: `5" pipe,10,X`   → col1 = "10"
        //   row1: `size 6",100,ok` → col1 = "100"
        //   row2: `A"B,3,C`        → col1 = "3"
        //
        // 수정 전 field_slice는 col0의 `"`에서 in_quotes를 켠 채 그 뒤 첫 콤마를
        // "인용 안"으로 오인해 삼켰다 → col1을 요청해도 실제로는 col0 전체(콤마
        // 포함 뒷부분까지 병합된 것)나 잘못된 컬럼을 가리켜, 숫자 파싱이 실패하고
        // NUM_INVALID(맨 뒤)가 되는 행이 생겼다. 수정 후에는 세 행 모두 col1이
        // 정확히 "10"/"100"/"3"으로 뽑힌다.
        //
        // 기대 순서: 숫자 오름차순 3 < 10 < 100 → row2, row0, row1 → [2, 0, 1].
        let (src, idx) =
            open_indexed(b"5\" pipe,10,X\nsize 6\",100,ok\nA\"B,3,C\n");
        let perm = extract_and_sort(
            &src,
            &idx,
            Encoding::Utf8,
            b',',
            1,
            0,
            SortKind::Number,
            SortDir::Asc,
            false,
            None,
        );
        assert_eq!(
            perm,
            vec![2, 0, 1],
            "필드 중간의 리터럴 \" 때문에 컬럼1 숫자 키가 어긋나면 안 된다"
        );
    }

    #[test]
    fn quoted_field_with_embedded_delimiter_keeps_column_boundaries() {
        // 정상적으로 인용된 필드(`"a,b"`)는 내부 콤마가 구분자가 아니라 필드의
        // 일부다. 따라서 컬럼 0은 `"a,b"` 전체, 컬럼 1은 그 다음 값이어야 한다.
        // 이 케이스는 인용 로직 자체(필드 첫 바이트에서만 인용 시작)가 여전히
        // 살아 있는지 확인한다 — 위 테스트가 "중간 따옴표는 무시"만 보장하고
        // "진짜 인용"까지 망가뜨리지 않았는지 함께 고정한다.
        //
        // 컬럼 1 값 하나만 있는 두 행으로 오름 정렬해 col1이 실제로 "2"/"5"임을
        // (즉 "b"로 오분류되지 않았음을) permutation으로 확인한다.
        //   row0: `"a,b",2` → col1 = "2"
        //   row1: `"c,d",5` → col1 = "5"
        // 숫자 오름: 2 < 5 → row0, row1 → [0, 1].
        let (src, idx) = open_indexed(b"\"a,b\",2\n\"c,d\",5\n");
        let perm = extract_and_sort(
            &src,
            &idx,
            Encoding::Utf8,
            b',',
            1,
            0,
            SortKind::Number,
            SortDir::Asc,
            false,
            None,
        );
        assert_eq!(
            perm,
            vec![0, 1],
            "인용된 필드 내부 콤마가 구분자로 오인되면 컬럼1이 \"b\"/\"d\"가 되어 숫자 파싱이 실패한다"
        );
    }

    // ---- 다중 컬럼 정렬 ----

    fn spec(col: usize, kind: SortKind, dir: SortDir) -> SortSpec {
        // 기존 다중 테스트는 대소문자 구분(ci=false)을 가정하고 짜였으므로 기본 false.
        SortSpec { col, kind, dir, ci: false }
    }

    fn spec_ci(col: usize, kind: SortKind, dir: SortDir, ci: bool) -> SortSpec {
        SortSpec { col, kind, dir, ci }
    }

    #[test]
    fn multi_city_then_age() {
        // city(col0 문자 오름), age(col1 숫자 오름).
        // 원본: 0 "B,30", 1 "A,20", 2 "B,10", 3 "A,40"
        // 1차 city: A(1,3), B(0,2). 2차 age 오름 → A: 20(1),40(3); B: 10(2),30(0)
        // → [1,3,2,0]
        let (src, idx) = open_indexed(b"B,30\nA,20\nB,10\nA,40\n");
        let specs = [
            spec(0, SortKind::Text, SortDir::Asc),
            spec(1, SortKind::Number, SortDir::Asc),
        ];
        let perm = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        assert_eq!(perm, vec![1, 3, 2, 0]);
    }

    #[test]
    fn multi_mixed_direction() {
        // 1차 city 오름, 2차 age 내림.
        // 원본: 0 "B,30", 1 "A,20", 2 "B,10", 3 "A,40"
        // city: A(1,3), B(0,2). age 내림 → A: 40(3),20(1); B: 30(0),10(2)
        // → [3,1,0,2]
        let (src, idx) = open_indexed(b"B,30\nA,20\nB,10\nA,40\n");
        let specs = [
            spec(0, SortKind::Text, SortDir::Asc),
            spec(1, SortKind::Number, SortDir::Desc),
        ];
        let perm = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        assert_eq!(perm, vec![3, 1, 0, 2]);
    }

    #[test]
    fn multi_first_key_tie_breaks_to_second() {
        // 1차가 모두 같으면 2차로 완전히 갈린다.
        // 원본: 0 "x,3", 1 "x,1", 2 "x,2" → 1차 x 동률, 2차 숫자 오름 1(1),2(2),3(0)
        // → [1,2,0]
        let (src, idx) = open_indexed(b"x,3\nx,1\nx,2\n");
        let specs = [
            spec(0, SortKind::Text, SortDir::Asc),
            spec(1, SortKind::Number, SortDir::Asc),
        ];
        let perm = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        assert_eq!(perm, vec![1, 2, 0]);
    }

    #[test]
    fn multi_non_numeric_last_in_key() {
        // 2차 숫자 기준에서 비수치는 그 그룹 내 맨 뒤(오름/내림 무관).
        // 원본: 0 "A,5", 1 "A,abc", 2 "A,1"
        // 1차 A 동률, 2차 숫자 오름: 1(2),5(0),[abc](1) → [2,0,1]
        let (src, idx) = open_indexed(b"A,5\nA,abc\nA,1\n");
        let specs = [
            spec(0, SortKind::Text, SortDir::Asc),
            spec(1, SortKind::Number, SortDir::Asc),
        ];
        let perm = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        assert_eq!(perm, vec![2, 0, 1]);
    }

    #[test]
    fn multi_non_numeric_last_even_descending() {
        // 2차 숫자 내림에서도 비수치는 맨 뒤.
        // 원본: 0 "A,5", 1 "A,abc", 2 "A,1"
        // 2차 내림: 5(0),1(2),[abc](1) → [0,2,1]
        let (src, idx) = open_indexed(b"A,5\nA,abc\nA,1\n");
        let specs = [
            spec(0, SortKind::Text, SortDir::Asc),
            spec(1, SortKind::Number, SortDir::Desc),
        ];
        let perm = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        assert_eq!(perm, vec![0, 2, 1]);
    }

    // ---- 부분집합 정렬(필터+정렬 조합용) ----

    #[test]
    fn subset_sort_only_orders_given_rows() {
        // 원본: 0 "c", 1 "a", 2 "b", 3 "d". rows=[0,2,1] (3은 필터에서 빠짐)
        // 문자 오름으로 정렬하면 a(1), b(2), c(0) → [1,2,0]. 3은 후보에도
        // 없으니 결과에 나오면 안 된다.
        let (src, idx) = open_indexed(b"c\na\nb\nd\n");
        let specs = [spec(0, SortKind::Text, SortDir::Asc)];
        let rows = [0u32, 2, 1];
        let perm = extract_and_multi_sort_subset(&src, &idx, Encoding::Utf8, b',', &specs, &rows, None);
        assert_eq!(perm, vec![1, 2, 0]);
    }

    #[test]
    fn subset_sort_empty_rows_returns_empty() {
        let (src, idx) = open_indexed(b"a\nb\n");
        let specs = [spec(0, SortKind::Text, SortDir::Asc)];
        let perm = extract_and_multi_sort_subset(&src, &idx, Encoding::Utf8, b',', &specs, &[], None);
        assert!(perm.is_empty());
    }

    #[test]
    fn subset_sort_matches_full_sort_when_rows_cover_everything() {
        // rows가 파일 전체(데이터 행 전부)를 담으면 결과가 전체 정렬과 같아야
        // 한다(회귀 — 부분집합 경로가 전체 경로와 같은 키 인코딩을 쓰는지).
        let content = b"B,30\nA,20\nB,10\nA,40\n";
        let (src, idx) = open_indexed(content);
        let specs = [
            spec(0, SortKind::Text, SortDir::Asc),
            spec(1, SortKind::Number, SortDir::Asc),
        ];
        let full = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        let all_rows: Vec<u32> = (0..4).collect();
        let subset =
            extract_and_multi_sort_subset(&src, &idx, Encoding::Utf8, b',', &specs, &all_rows, None);
        assert_eq!(full, subset);
    }

    #[test]
    fn multi_single_spec_matches_single_sort() {
        // 기준 1개면 단일 정렬과 동일 결과(회귀).
        let (src, idx) = open_indexed(b"10\n2\n9\n");
        let single =
            extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Number, SortDir::Asc, false, None);
        let multi = extract_and_multi_sort(
            &src,
            &idx,
            Encoding::Utf8,
            b',',
            &[spec(0, SortKind::Number, SortDir::Asc)],
            0,
            None,
        );
        assert_eq!(single, multi);
    }

    #[test]
    fn multi_empty_specs_returns_empty() {
        let (src, idx) = open_indexed(b"a\nb\n");
        let perm = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &[], 0, None);
        assert!(perm.is_empty());
    }

    // ---- case-insensitive ----

    #[test]
    fn single_text_case_sensitive_vs_insensitive() {
        // 원본: 0 "banana", 1 "Apple", 2 "cherry"
        // 대소문자 구분(ci=false, 바이트순): 'A'(0x41) < 'b'(0x62) < 'c'(0x63)
        //   → Apple(1), banana(0), cherry(2) = [1,0,2]
        // 대소문자 무시(ci=true, 소문자화): apple < banana < cherry
        //   → Apple(1), banana(0), cherry(2) = [1,0,2] (이 데이터는 동일)
        // 구분이 실제로 갈리는 케이스: "Zebra" vs "apple"
        //   구분: 'Z'(0x5A) < 'a'(0x61) → Zebra 먼저. 무시: apple < zebra → apple 먼저.
        let (src, idx) = open_indexed(b"Zebra\napple\n");
        let cs = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, false, None);
        assert_eq!(cs, vec![0, 1], "구분: 대문자 Z가 소문자 a보다 앞");
        let ci = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, true, None);
        assert_eq!(ci, vec![1, 0], "무시: apple이 zebra보다 앞");
    }

    #[test]
    fn ci_groups_mixed_case_together() {
        // 대소문자 무시면 "Apple"과 "apple"이 같은 그룹(동률) → 원본 순서 유지.
        // 원본: 0 "apple", 1 "Banana", 2 "Apple", 3 "banana"
        // 무시 오름: apple/Apple 그룹(0,2), banana/Banana 그룹(1,3)
        //   각 그룹 내 안정 정렬(원본 행번호) → [0,2,1,3]
        let (src, idx) = open_indexed(b"apple\nBanana\nApple\nbanana\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, true, None);
        assert_eq!(perm, vec![0, 2, 1, 3]);
    }

    #[test]
    fn ci_tie_break_beyond_prefix() {
        // 앞 8바이트가 대소문자만 달라 ci 키는 같고, 9번째 이후에서 갈리는 경우
        // tie-break가 소문자화해 비교하는지. prefix "ABCDEFGH"(8자, ci 동률).
        // row0 = ABCDEFGH_z, row1 = abcdefgh_a → 무시: ..._a < ..._z → [1,0]
        let (src, idx) = open_indexed(b"ABCDEFGH_z\nabcdefgh_a\n");
        let perm = extract_and_sort(&src, &idx, Encoding::Utf8, b',', 0, 0, SortKind::Text, SortDir::Asc, true, None);
        assert_eq!(perm, vec![1, 0]);
    }

    #[test]
    fn multi_ci_spec() {
        // 다중에서 문자 기준 ci: "Zebra" vs "apple" 무시 → apple 먼저.
        let (src, idx) = open_indexed(b"Zebra\napple\n");
        let specs = [spec_ci(0, SortKind::Text, SortDir::Asc, true)];
        let perm = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        assert_eq!(perm, vec![1, 0]);
    }

    // ---- 인메모리 정렬(sort_lines) ----

    #[test]
    fn sort_lines_text_ascending() {
        let lines = vec![
            "banana".to_string(),
            "apple".to_string(),
            "cherry".to_string(),
        ];
        let specs = [spec(0, SortKind::Text, SortDir::Asc)];
        let order = sort_lines(&lines, &specs, b',', 0);
        assert_eq!(order, vec![1, 0, 2]);
    }

    #[test]
    fn sort_lines_multi_matches_mmap_path() {
        // 인메모리 정렬이 mmap 경로(extract_and_multi_sort)와 같은 결과.
        let content = b"B,30\nA,20\nB,10\nA,40\n";
        let (src, idx) = open_indexed(content);
        let specs = [
            spec(0, SortKind::Text, SortDir::Asc),
            spec(1, SortKind::Number, SortDir::Asc),
        ];
        let mmap_order = extract_and_multi_sort(&src, &idx, Encoding::Utf8, b',', &specs, 0, None);
        let lines: Vec<String> = "B,30\nA,20\nB,10\nA,40"
            .split('\n')
            .map(|s| s.to_string())
            .collect();
        let mem_order = sort_lines(&lines, &specs, b',', 0);
        assert_eq!(mmap_order, mem_order);
    }

    #[test]
    fn sort_lines_respects_data_start() {
        // data_start=1이면 헤더(0행) 제외, 데이터만 정렬한 논리 행번호 반환.
        let lines = vec![
            "name".to_string(),
            "banana".to_string(),
            "apple".to_string(),
        ];
        let specs = [spec(0, SortKind::Text, SortDir::Asc)];
        let order = sort_lines(&lines, &specs, b',', 1);
        // apple(2), banana(1) → [2,1]
        assert_eq!(order, vec![2, 1]);
    }

    #[test]
    fn sort_lines_subset_only_orders_given_rows() {
        // 원본: 0 "c", 1 "a", 2 "b", 3 "d". rows=[0,2,1] (3은 대상 밖).
        let lines = vec!["c".to_string(), "a".to_string(), "b".to_string(), "d".to_string()];
        let specs = [spec(0, SortKind::Text, SortDir::Asc)];
        let rows = [0u32, 2, 1];
        let order = sort_lines_subset(&lines, &specs, b',', &rows);
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn sort_lines_subset_empty_rows_is_empty() {
        let lines = vec!["a".to_string()];
        let specs = [spec(0, SortKind::Text, SortDir::Asc)];
        assert!(sort_lines_subset(&lines, &specs, b',', &[]).is_empty());
    }
}
