use crate::parse::{decode_line, Encoding};
use crate::source::Source;
use crate::parse::{join_fields, split_fields};

/// 원본 파일의 개행 스타일. 저장 시 재현한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newline {
    Lf,
    CrLf,
}

/// 편집 모드의 인메모리 문서. 파일 전체를 줄 단위로 보관한다.
/// lines[i]에는 개행 문자가 포함되지 않는다.
pub struct EditBuffer {
    pub lines: Vec<String>,
    pub dirty: bool,
    pub newline: Newline,
    /// 되돌리기 스택(최근 UNDO_LIMIT 단계).
    pub undo: UndoStack,
}

/// mmap 바이트를 개행(`\n`)으로 분할하고 각 줄을 `enc`로 디코딩해 EditBuffer를 만든다.
/// - `\r\n`이 하나라도 있으면 newline=CrLf, 아니면 Lf.
/// - 각 줄에서 뒤쪽 `\r`/`\n`을 제거한다.
/// - 마지막 바이트가 개행이면 그 뒤에 빈 줄을 추가하지 않는다(파일 끝 개행은 종결자).
/// - 빈 파일이면 `lines = [""]`(빈 한 줄).
pub fn load_edit_buffer(source: &Source, enc: Encoding) -> EditBuffer {
    let bytes = source.as_bytes();
    if bytes.is_empty() {
        return EditBuffer {
            lines: vec![String::new()],
            dirty: false,
            newline: Newline::Lf,
            undo: UndoStack::new(UNDO_LIMIT),
        };
    }
    let mut newline = Newline::Lf;
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(pos) = memchr::memchr(b'\n', &bytes[i..]) {
            let nl = i + pos; // '\n' 위치
            let mut end = nl;
            if end > start && bytes[end - 1] == b'\r' {
                end -= 1;
                newline = Newline::CrLf;
            }
            lines.push(decode_line(&bytes[start..end], enc));
            start = nl + 1;
            i = nl + 1;
        } else {
            break;
        }
    }
    // 마지막 개행 뒤에 내용이 남아 있으면(파일 끝 개행 없음) 그 줄도 추가.
    if start < bytes.len() {
        let mut end = bytes.len();
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
            newline = Newline::CrLf;
        }
        lines.push(decode_line(&bytes[start..end], enc));
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    EditBuffer { lines, dirty: false, newline, undo: UndoStack::new(UNDO_LIMIT) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPos {
    pub line: usize,
    pub col: usize, // 문자(char) 인덱스
}

/// 문자열의 char 인덱스 col을 바이트 오프셋으로 변환(끝이면 len).
fn byte_of(s: &str, col: usize) -> usize {
    s.char_indices()
        .nth(col)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// 한 줄의 char 개수.
fn char_len(s: &str) -> usize {
    s.chars().count()
}

pub fn normalize(a: TextPos, b: TextPos) -> (TextPos, TextPos) {
    if (a.line, a.col) <= (b.line, b.col) {
        (a, b)
    } else {
        (b, a)
    }
}

pub fn insert_char(lines: &mut Vec<String>, pos: TextPos, ch: char) -> TextPos {
    let b = byte_of(&lines[pos.line], pos.col);
    lines[pos.line].insert(b, ch);
    TextPos { line: pos.line, col: pos.col + 1 }
}

pub fn split_line(lines: &mut Vec<String>, pos: TextPos) -> TextPos {
    let b = byte_of(&lines[pos.line], pos.col);
    let tail = lines[pos.line].split_off(b);
    lines.insert(pos.line + 1, tail);
    TextPos { line: pos.line + 1, col: 0 }
}

pub fn insert_str(lines: &mut Vec<String>, pos: TextPos, s: &str) -> TextPos {
    // s를 개행으로 나눠 삽입. 개행 없으면 단순 삽입.
    let mut parts = s.split('\n');
    let first = parts.next().unwrap_or("");
    let b = byte_of(&lines[pos.line], pos.col);
    // 현재 줄을 삽입 지점에서 자른다.
    let tail = lines[pos.line].split_off(b);
    lines[pos.line].push_str(first);
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        // 개행 없음: tail을 다시 붙이고 커서는 first 끝.
        let col = pos.col + char_len(first);
        lines[pos.line].push_str(&tail);
        return TextPos { line: pos.line, col };
    }
    // 개행 있음: 중간 줄들 삽입, 마지막 줄에 tail 붙임.
    let mut cur = pos.line;
    for (k, seg) in rest.iter().enumerate() {
        cur += 1;
        if k + 1 == rest.len() {
            let mut last = seg.to_string();
            let col = char_len(&last);
            last.push_str(&tail);
            lines.insert(cur, last);
            return TextPos { line: cur, col };
        } else {
            lines.insert(cur, seg.to_string());
        }
    }
    unreachable!()
}

pub fn delete_range(lines: &mut Vec<String>, a: TextPos, b: TextPos) -> TextPos {
    let (a, b) = normalize(a, b);
    if a == b {
        return a;
    }
    if a.line == b.line {
        let sa = byte_of(&lines[a.line], a.col);
        let sb = byte_of(&lines[a.line], b.col);
        lines[a.line].replace_range(sa..sb, "");
        return a;
    }
    // 멀티라인: a.line의 앞부분 + b.line의 뒷부분 병합, 사이 줄 제거.
    let sa = byte_of(&lines[a.line], a.col);
    let head = lines[a.line][..sa].to_string();
    let sb = byte_of(&lines[b.line], b.col);
    let tail = lines[b.line][sb..].to_string();
    let merged = head + &tail;
    lines[a.line] = merged;
    // a.line+1 ..= b.line 제거.
    lines.drain(a.line + 1..=b.line);
    a
}

/// [a, b) 범위의 텍스트를 추출한다(`delete_range`의 대칭). 정규화 후
/// 시작 줄 뒷부분 + 중간 줄 전체 + 끝 줄 앞부분을 `\n`으로 join한다.
/// 빈 범위(a == b)면 빈 문자열.
pub fn selection_text(lines: &[String], a: TextPos, b: TextPos) -> String {
    let (a, b) = normalize(a, b);
    if a == b || a.line >= lines.len() {
        return String::new();
    }
    if a.line == b.line {
        let s = &lines[a.line];
        let sa = byte_of(s, a.col);
        let sb = byte_of(s, b.col);
        return s[sa.min(sb)..sb.max(sa)].to_owned();
    }
    let mut out = String::new();
    // 시작 줄의 뒷부분.
    let sa = byte_of(&lines[a.line], a.col);
    out.push_str(&lines[a.line][sa..]);
    // 중간 줄 전체.
    let last = b.line.min(lines.len().saturating_sub(1));
    for l in (a.line + 1)..last {
        out.push('\n');
        out.push_str(&lines[l]);
    }
    // 끝 줄의 앞부분.
    if last > a.line {
        out.push('\n');
        let sb = byte_of(&lines[last], b.col);
        out.push_str(&lines[last][..sb]);
    }
    out
}

/// 더블클릭 선택 범위. `col` 위치를 감싸는 "공백으로 구분된 덩어리"의
/// [시작, 끝) char 인덱스를 돌려준다.
///
/// 경계는 **공백(스페이스/탭 등 `char::is_whitespace`)뿐**이다. 흔한
/// 에디터가 쓰는 "낱말 문자 vs 구두점" 구분은 쓰지 않는다 — 이 프로그램이
/// 다루는 것은 주로 CSV/TSV 한 줄이고, 거기서 사람이 잡고 싶은 단위는
/// `2026-08-27` 이나 `a.b.c` 같은 필드 통째이지 그 조각이 아니다.
///
/// 공백 위에서 눌렀다면 그 공백 덩어리를 돌려준다(Windows 표준과 같다 —
/// 빈 선택으로 무너뜨리지 않는다).
///
/// 빈 줄이면 `(0, 0)`.
pub fn word_range_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    // 줄 끝에서 눌렀으면 마지막 문자를 기준으로 삼는다. 그러지 않으면
    // 줄 끝 더블클릭이 늘 빈 선택이 된다.
    let at = col.min(chars.len() - 1);
    let ws = chars[at].is_whitespace();

    let mut start = at;
    while start > 0 && chars[start - 1].is_whitespace() == ws {
        start -= 1;
    }
    let mut end = at + 1;
    while end < chars.len() && chars[end].is_whitespace() == ws {
        end += 1;
    }
    (start, end)
}

pub fn backspace(lines: &mut Vec<String>, pos: TextPos) -> TextPos {
    if pos.col > 0 {
        let prev = TextPos { line: pos.line, col: pos.col - 1 };
        return delete_range(lines, prev, pos);
    }
    if pos.line == 0 {
        return pos; // 문서 맨 앞: no-op
    }
    // 줄 맨 앞: 앞 줄과 병합. 병합점 = 앞 줄의 기존 끝.
    let prev_len = char_len(&lines[pos.line - 1]);
    let merge = TextPos { line: pos.line - 1, col: prev_len };
    delete_range(lines, merge, pos)
}

/// logical 행의 col번째 필드를 value로 교체하고 줄을 재조립한다.
/// col이 현재 필드 수보다 크면 빈 필드로 패딩한다.
/// 셀 값은 한 행에 속하므로 개행을 포함할 수 없다 — 개행은 공백으로 치환해
/// lines[i] 불변식(개행 없음)을 보장한다.
pub fn set_cell(lines: &mut [String], logical: usize, col: usize, value: &str, delim: u8) {
    let value = sanitize_cell_value(value);
    let mut fields = split_fields(&lines[logical], delim);
    if col >= fields.len() {
        fields.resize(col + 1, String::new());
    }
    fields[col] = value;
    lines[logical] = join_fields(&fields, delim);
}

/// 셀 값에 포함된 `\n`/`\r`를 공백으로 치환한다. 셀은 한 행에 속하므로
/// 개행을 담을 수 없다 — lines[i]에 개행이 박히는 것을 방지한다.
fn sanitize_cell_value(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

/// 사각 영역 [r0..=r1] x [c0..=c1]의 각 셀을 빈 값으로 만든다.
///
/// 성능 주의: 컬럼 선택은 데이터 전 구간(수억 행)을 범위로 만들 수 있다. 값을
/// 비우려면 줄을 재조립해야 하므로 `split_fields`+`join_fields`가 필요하지만,
/// **바꿀 게 없는 행은 통째로 건너뛴다** — 대상 필드가 이미 전부 비어 있으면
/// `field_slice`(할당 없음)만으로 판정하고 재조립을 생략한다.
pub fn clear_cells(lines: &mut [String], r0: usize, c0: usize, r1: usize, c1: usize, delim: u8) {
    let (r0, r1) = (r0.min(r1), r0.max(r1));
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    for r in r0..=r1 {
        if r >= lines.len() {
            break;
        }
        // 이미 전부 비어 있으면 재조립(할당) 자체를 건너뛴다.
        let already_empty = {
            let bytes = lines[r].as_bytes();
            (c0..=c1)
                .all(|c| crate::parse::field_slice(bytes, delim, c).map_or(true, |f| f.is_empty()))
        };
        if already_empty {
            continue;
        }
        let mut fields = split_fields(&lines[r], delim);
        let hi = c1.min(fields.len().saturating_sub(1));
        for c in c0..=hi {
            if c < fields.len() {
                fields[c].clear();
            }
        }
        lines[r] = join_fields(&fields, delim);
    }
}

/// 선택 사각 영역을 TSV(행=\n, 열=\t)로 직렬화한다.
///
/// 성능 주의: 컬럼 선택(헤더 클릭)은 데이터 전 구간을 범위로 만든다 —
/// 3억 행이면 이 루프가 3억 번 돈다. 그래서 행마다 `split_fields`로
/// **모든** 필드를 `String`으로 만들지 않고(리더 생성 + Vec 2개 + 필드 수만큼
/// String 할당), 필요한 컬럼만 `parse::field_slice`로 **할당 없이** 잘라 온다.
/// `field_slice`는 raw 슬라이스(따옴표 포함)를 주므로 `unquote_field`로
/// `split_fields`와 같은 값이 되도록 인용을 벗긴다.
pub fn cells_to_tsv(lines: &[String], r0: usize, c0: usize, r1: usize, c1: usize, delim: u8) -> String {
    let (r0, r1) = (r0.min(r1), r0.max(r1));
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    // 결과 크기를 대략 예측해 재할당을 줄인다(행당 평균 16바이트 가정).
    let mut out = String::with_capacity((r1.saturating_sub(r0) + 1).saturating_mul(16));
    for r in r0..=r1 {
        if r > r0 {
            out.push('\n');
        }
        if r >= lines.len() {
            continue;
        }
        let bytes = lines[r].as_bytes();
        for c in c0..=c1 {
            if c > c0 {
                out.push('\t');
            }
            // 할당 없이 해당 필드 슬라이스만. 따옴표로 감싼 값은 그대로
            // 포함되므로 여기서 벗겨 준다(split_fields와 결과를 맞추기 위함).
            if let Some(f) = crate::parse::field_slice(bytes, delim, c) {
                push_unquoted(&mut out, f);
            }
        }
    }
    out
}

/// `field_slice`가 돌려준 raw 필드에서 CSV 인용을 벗겨 `out`에 덧붙인다.
///
/// **csv_core(= `split_fields`)의 동작을 그대로 재현한다.** 브리프가 제안한
/// "앞뒤가 `"`면 벗기고 내부 `""`를 하나로" 규칙은 실제 csv_core와 다음
/// 입력들에서 어긋나므로 채택하지 않았다(모두 실측):
///   `"`      → csv_core `""`,      단순규칙 `"\""`   (len<2라 안 벗김)
///   `"ab`    → csv_core `"ab"`,    단순규칙 `"\"ab"` (끝이 따옴표가 아님)
///   `"a"b"`  → csv_core `"ab\""`,  단순규칙 `"a\"b"` (중간 따옴표에서 인용 종료)
///   `"a"b`   → csv_core `"ab"`,    단순규칙 그대로
/// 실제 규칙은 상태기계다:
///   - 필드가 `"`로 시작하지 않으면 **전부 그대로**(내부 `"`도 리터럴).
///   - `"`로 시작하면 인용 상태로 진입하고 그 여는 따옴표는 버린다.
///     인용 안에서 `""`는 `"` 하나로, 홀로 있는 `"`는 인용을 **닫는다**.
///     인용을 닫은 뒤의 바이트는 다시 리터럴이다(닫는 따옴표 자체는 버려짐).
/// 할당은 하지 않는다 — 바이트 구간을 그대로 `out`에 복사한다.
fn push_unquoted(out: &mut String, f: &[u8]) {
    if !f.starts_with(b"\"") {
        out.push_str(&String::from_utf8_lossy(f));
        return;
    }
    let mut i = 1usize; // 여는 따옴표는 버린다
    let mut in_quotes = true;
    let mut seg_start = i;
    while i < f.len() {
        if in_quotes && f[i] == b'"' {
            // 여기까지의 구간을 흘려보내고 따옴표를 처리한다.
            out.push_str(&String::from_utf8_lossy(&f[seg_start..i]));
            if f.get(i + 1) == Some(&b'"') {
                out.push('"'); // "" → " (인용 유지)
                i += 2;
            } else {
                in_quotes = false; // 인용 종료(닫는 따옴표는 버림)
                i += 1;
            }
            seg_start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&String::from_utf8_lossy(&f[seg_start..]));
}

/// TSV 클립보드를 (r0,c0)부터 셀 그리드로 덮어쓴다. 경계를 넘으면 행/열 확장.
/// 붙여넣는 값은 set_cell과 동일하게 sanitize_cell_value를 거쳐 lines[i] 개행 불변식을 지킨다.
pub fn paste_tsv(lines: &mut Vec<String>, r0: usize, c0: usize, tsv: &str, delim: u8) {
    for (dr, row) in tsv.split('\n').enumerate() {
        let r = r0 + dr;
        while r >= lines.len() {
            lines.push(String::new());
        }
        let mut fields = split_fields(&lines[r], delim);
        for (dc, cell) in row.split('\t').enumerate() {
            let c = c0 + dc;
            if c >= fields.len() {
                fields.resize(c + 1, String::new());
            }
            fields[c] = sanitize_cell_value(cell);
        }
        lines[r] = join_fields(&fields, delim);
    }
}

pub fn insert_row(lines: &mut Vec<String>, at: usize, text: String) {
    let at = at.min(lines.len());
    lines.insert(at, text);
}

pub fn remove_row(lines: &mut Vec<String>, at: usize) {
    if at < lines.len() {
        lines.remove(at);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
}

/// order(원본 논리 행번호 배열) 순서로 데이터 행을 재배치한다. data_start 이전
/// 행(헤더)은 그대로 앞에 유지한다. order는 데이터 행만 커버한다.
///
/// **행 텍스트를 복사하지 않고 옮긴다(`mem::take`).** 예전에는 `clone()`이라
/// 정렬 한 번마다 파일 전체를 통째로 재할당했다 — 15.4M행 실파일에서
/// **785ms → 97ms(8.1배)**, 정렬 중 최고 상주 메모리도 4,602MB → 3,627MB로
/// 준다(버퍼 사본이 잠깐 두 벌 뜨던 것이 없어졌다).
///
/// **같은 인덱스가 order에 두 번 나오면 두 번째부터는 `clone`으로 돌아간다.**
/// 정상 순열이면 있을 수 없는 일이지만, `inverse_of`는 order가 순열이 아닐 때
/// 안 채워진 자리에 0을 남기므로(그 함수의 `vec![0u32; ..]` 참조) 중복 인덱스가
/// 들어올 수 있다. 그때 무턱대고 `take`하면 두 번째 자리가 빈 줄이 되어
/// **데이터가 조용히 사라진다**. 아래 `taken` 표로 첫 방문만 옮기고 나머지는
/// 복사해, 결과가 옛 `clone` 판과 어떤 입력에서도 글자 그대로 같도록 유지한다.
/// 표는 행당 `usize` 하나(15.4M행에 123MB)를 쓰지만 이 함수가 도는 동안만
/// 살아 있고, 그래도 전 행을 `clone`하던 옛 판이 만들던 사본보다 훨씬 싸다.
pub fn apply_permutation(lines: &mut Vec<String>, order: &[u32], data_start: usize) {
    if data_start > lines.len() {
        return;
    }
    let n = lines.len();
    // taken[i] = 원본 i행을 이미 꺼내 담은 out의 자리(+1). 0이면 아직 안 꺼냈다.
    // Option<usize> 대신 +1 인코딩을 쓰는 이유는 15.4M행에서 8바이트/행이라도
    // 아끼려는 게 아니라, 0 초기화가 vec![]의 memset 한 번으로 끝나기 때문이다.
    let mut taken = vec![0usize; n];
    let mut out: Vec<String> = Vec::with_capacity(data_start + order.len());
    // 헤더는 순열 대상이 아니므로 앞에서부터 그대로 꺼내 옮긴다. 옮긴 자리를
    // taken에 등록해 두는 이유: order가 헤더 인덱스를 가리키는 경우(정상 정렬에선
    // 없지만 `inverse_of`의 0 채움으로 생길 수 있다) 원본이 이미 비어 있으므로,
    // 아래 루프가 빈 줄을 담지 않고 이 자리에서 복사해 오게 해야 한다.
    for (i, slot) in lines.iter_mut().take(data_start).enumerate() {
        taken[i] = i + 1;
        out.push(std::mem::take(slot));
    }
    for &idx in order {
        let i = idx as usize;
        if i >= n {
            continue;
        }
        if taken[i] == 0 {
            taken[i] = out.len() + 1;
            out.push(std::mem::take(&mut lines[i]));
        } else {
            // 중복 인덱스 — 원본은 이미 비었으니 먼저 옮겨 둔 자리에서 복사한다.
            out.push(out[taken[i] - 1].clone());
        }
    }
    *lines = out;
}

/// 되돌리기 한 단계. 각 variant는 그 편집을 취소하는 데 필요한 최소 정보를 담는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOp {
    /// 여러 줄의 이전 내용을 복원한다(셀 편집·범위 지우기·붙여넣기 등 값 변경).
    /// (논리 행번호, 그 행의 이전 전체 텍스트) 목록.
    Replace(Vec<(usize, String)>),
    /// 삽입된 행들을 제거한다. at부터 count개를 지우면 원상복구.
    RemoveInserted { at: usize, count: usize },
    /// 삭제된 행들을 되살린다. at 위치에 lines를 다시 끼워 넣는다.
    ReinsertRemoved { at: usize, lines: Vec<String> },
    /// 정렬 등 전체 재배치를 되돌린다. inverse[i] = 재배치 후 i번째 줄이
    /// 원래 있던 위치. data_start 이전(헤더)은 건드리지 않는다.
    Reorder { inverse: Vec<u32>, data_start: usize },
    /// 여러 op를 **한 단계**로 묶는다. 한 번의 사용자 동작이 구조 변화와 값
    /// 변화를 동시에 일으킬 때(예: 붙여넣기로 행이 늘어남, Enter로 줄이 갈라짐)
    /// Ctrl+Z 한 번에 전부 되돌아가야 하기 때문에 필요하다.
    /// 내부 op는 **앞에서부터 순서대로** 적용된다 — 즉 `Batch(vec![a, b])`는
    /// a를 먼저, b를 나중에 되돌린다(스택의 LIFO와 반대 방향이므로,
    /// 만드는 쪽이 "되돌릴 순서"대로 담으면 된다).
    Batch(Vec<EditOp>),
}

/// 되돌리기 스택. **단계 수와 바이트 예산 둘 다**로 자르고, 넘으면 가장
/// 오래된 것부터 버린다(FIFO). 둘 중 먼저 걸리는 쪽이 이긴다.
pub struct UndoStack {
    ops: Vec<EditOp>,
    /// `ops[i]`의 대략적인 바이트(`op_bytes`). push 때 한 번만 재고 들고 다닌다 —
    /// 버릴 개수를 정할 때마다 다시 세면 스택 전체를 훑게 되는데, 그게 바로
    /// 이 예산이 막으려는 규모(수백 MB 문자열)라 자기모순이 된다.
    sizes: Vec<usize>,
    limit: usize,
    /// `sizes`의 합. 증감으로만 유지한다.
    bytes: usize,
    budget: usize,
    /// 편집이 일어날 때마다 1 늘어나는 카운터. 되돌리기도 편집이므로 함께 센다
    /// (같은 값으로 되돌아가지 않는다 — 되돌린 뒤의 내용은 그 전과 다르다).
    ///
    /// **용도: 캐시 무효화.** 오류 행 목록처럼 "이 시점의 데이터"를 설명하는
    /// 결과가, 데이터가 바뀐 뒤에도 최신인 척 남아 있으면 안 된다.
    ///
    /// **왜 `dirty`나 스택 길이가 아니라 이것인가.** `dirty`는 한 번 서면 계속
    /// 서 있어 "그 뒤로 또 바뀌었는가"를 답하지 못한다. 스택 길이는 상한
    /// (`UNDO_LIMIT`·바이트 예산)에 걸리면 push를 해도 그대로거나 줄어들 수
    /// 있어 편집을 놓친다.
    ///
    /// **왜 편집 지점마다 세지 않고 여기서 세는가.** 편집을 수행하는 자리는
    /// `app.rs`에 열 곳이 넘는데, 전부가 예외 없이 `push`를 지난다. 지점마다
    /// 손으로 세면 나중에 추가되는 편집 하나가 조용히 빠지고, 그때 증상은
    /// "오류 목록이 가끔 안 맞는다"라 추적하기 가장 나쁜 종류다.
    revision: u64,
}

/// 기본 되돌리기 깊이(사용자 확정).
pub const UNDO_LIMIT: usize = 100;

/// 되돌리기 스택이 붙들 수 있는 대략적인 최대 바이트.
///
/// **단계 수 제한만으로는 부족하다.** 15.4M행 파일에서 컬럼 연산을 100번 하면
/// `UNDO_LIMIT = 100`은 아무것도 안 막고 100 × 900MB를 붙들어 수십 GB가 된다.
/// 한 단계가 파일 전체를 담을 수 있는 이상, 단계 수는 메모리 상한이 아니다.
///
/// 값의 근거: 이 앱은 수 GB 파일을 여는 것이 전제라 버퍼 자체가 이미 수 GB를
/// 쓴다. 되돌리기가 그 위에 얹을 수 있는 몫으로 512MB면 실사용에서 넉넉하고
/// (전체 바꾸기 한 번의 undo가 이 파일에서 ~370MB), 32GB 기계에서도 스왑을
/// 유발하지 않는다. 더 키우면 큰 파일에서 되돌리기 한두 단계 때문에 앱이
/// 통째로 느려질 위험이 커지고, 더 줄이면 큰 파일에서 전체 바꾸기 한 번조차
/// 못 되돌린다.
pub const UNDO_BYTE_BUDGET: usize = 512 * 1024 * 1024;

/// op 하나가 붙들고 있는 대략적인 힙 바이트.
///
/// **정확할 필요가 없다 — 자릿수만 맞으면 된다.** 목적은 회계가 아니라 "수십
/// GB를 조용히 붙들고 있는 상태"를 막는 것이다. 그래서 `String`의 여유 용량이나
/// `Vec`의 초과 할당은 세지 않고, 실제로 담긴 내용 + 원소당 고정 비용만 센다.
/// 반대로 **과소평가는 하지 않는다** — 원소가 많고 내용이 짧은 op(예: 빈 줄
/// 100만 개)도 포인터·길이만으로 수십 MB이므로 그 고정 비용을 반드시 더한다.
pub fn op_bytes(op: &EditOp) -> usize {
    // (usize, String) 한 칸과 String 한 칸의 고정 비용. String은 포인터+길이+용량
    // 24바이트, 앞의 usize까지 32바이트.
    const PAIR: usize = 32;
    const STR: usize = 24;
    match op {
        EditOp::Replace(items) => {
            items.len() * PAIR + items.iter().map(|(_, s)| s.len()).sum::<usize>()
        }
        // 지운 행을 되살릴 정보가 없다 — 위치와 개수뿐이라 사실상 공짜.
        EditOp::RemoveInserted { .. } => 0,
        EditOp::ReinsertRemoved { lines, .. } => {
            lines.len() * STR + lines.iter().map(|s| s.len()).sum::<usize>()
        }
        EditOp::Reorder { inverse, .. } => inverse.len() * std::mem::size_of::<u32>(),
        EditOp::Batch(ops) => ops.iter().map(op_bytes).sum(),
    }
}

/// 새 op를 쌓기 **전에** 앞에서 몇 개를 버려야 하는지 정한다.
///
/// 순수 함수로 뽑아 둔 이유는 이 코드베이스의 규율이다 — 판단식을 `push` 안에
/// 인라인해 두면 테스트가 스택 상태를 통째로 꾸며야 검증할 수 있고, 규칙을
/// 뒤집어도 테스트가 눈치채지 못한다.
///
/// 규칙:
/// - `limit == 0`이면 애초에 아무것도 안 쌓으므로 이 함수를 부르지 않는다.
/// - 단계 수가 `limit`에 닿으면 자리를 만들려고 최소 하나는 버린다.
/// - 그러고도 (남은 바이트 + 새 op)가 예산을 넘으면 넘지 않을 때까지 더 버린다.
/// - **새 op 하나가 혼자 예산을 넘겨도 전부 버리고 그 하나는 남긴다.** 되돌리기
///   자체를 포기시키는 것보다 낫고, 어차피 다음 push가 이걸 밀어낸다.
///
/// 반환값은 "앞에서 버릴 개수"이며 `existing.len()`을 넘지 않는다.
pub fn undo_evict_count(existing: &[usize], new_bytes: usize, limit: usize, budget: usize) -> usize {
    let mut drop = if existing.len() >= limit {
        // 단계 수 초과분 + 자리 하나.
        existing.len() - limit + 1
    } else {
        0
    };
    let mut kept: usize = existing.iter().skip(drop).sum();
    while drop < existing.len() && kept + new_bytes > budget {
        kept -= existing[drop];
        drop += 1;
    }
    drop
}

impl UndoStack {
    pub fn new(limit: usize) -> Self {
        Self::with_budget(limit, UNDO_BYTE_BUDGET)
    }

    /// 바이트 예산을 직접 정해 만든다(테스트용 — 512MB를 실제로 채우려면
    /// 테스트가 수백 MB를 할당해야 한다).
    pub fn with_budget(limit: usize, budget: usize) -> Self {
        UndoStack { ops: Vec::new(), sizes: Vec::new(), limit, bytes: 0, budget, revision: 0 }
    }

    /// 지금까지 일어난 편집 횟수. 값 자체에는 의미가 없고 **달라졌는지**만 본다
    /// (`revision` 필드 주석 참조).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 지금 쌓인 op들의 대략적인 바이트 합.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// 되돌리기 한 단계를 쌓는다. 단계 수나 바이트 예산을 넘으면 가장 오래된
    /// 것부터 버린다. **편집을 적용하기 직전에** 호출해야 한다(이전 상태를 담으므로).
    pub fn push(&mut self, op: EditOp) {
        // 되돌리기를 못 쌓는 경우(limit 0)에도 **편집은 일어났다**. 아래
        // 조기 반환보다 먼저 세지 않으면 그 편집이 캐시 무효화에서 누락된다.
        self.revision += 1;
        if self.limit == 0 {
            return;
        }
        let new_bytes = op_bytes(&op);
        let drop = undo_evict_count(&self.sizes, new_bytes, self.limit, self.budget);
        if drop > 0 {
            self.bytes -= self.sizes.iter().take(drop).sum::<usize>();
            self.ops.drain(..drop);
            self.sizes.drain(..drop);
        }
        self.bytes += new_bytes;
        self.ops.push(op);
        self.sizes.push(new_bytes);
    }

    pub fn clear(&mut self) {
        self.ops.clear();
        self.sizes.clear();
        self.bytes = 0;
    }

    /// 가장 최근 편집을 되돌린다. 되돌릴 게 없으면 false.
    /// 범위를 벗어난 인덱스는 조용히 건너뛴다(패닉 없음).
    pub fn undo(&mut self, lines: &mut Vec<String>) -> bool {
        let Some(op) = self.ops.pop() else {
            // 되돌릴 게 없으면 내용도 안 바뀌었다 — revision을 올리지 않는다.
            return false;
        };
        // 되돌리기도 내용을 바꾸므로 편집이다.
        self.revision += 1;
        // 되돌린 단계는 스택에서 빠지므로 회계에서도 빼야 한다 — 안 그러면
        // undo를 반복할수록 남은 예산이 실제보다 작다고 착각한다.
        if let Some(b) = self.sizes.pop() {
            self.bytes -= b;
        }
        apply_undo_op(lines, op);
        // "lines는 비어 있지 않다" 불변식은 **한 단계가 끝난 뒤**에만 강제한다.
        // Batch 중간에 강제하면 유령 줄이 다시 생겨(제거 → 즉시 재삽입) 뒤이은
        // 되꽂기가 한 줄 늘어난 결과를 낳는다.
        if lines.is_empty() {
            lines.push(String::new());
        }
        true
    }
}

/// op 하나를 lines에 적용한다(되돌리기 실행). `Batch`는 담긴 순서대로 재귀 적용.
/// 빈 lines 방어는 호출측(`UndoStack::undo`)이 한 단계 끝에 한 번만 한다.
fn apply_undo_op(lines: &mut Vec<String>, op: EditOp) {
    match op {
        EditOp::Replace(items) => {
            for (row, text) in items {
                if row < lines.len() {
                    lines[row] = text;
                }
            }
        }
        EditOp::RemoveInserted { at, count } => {
            let end = (at + count).min(lines.len());
            if at < end {
                lines.drain(at..end);
            }
        }
        EditOp::ReinsertRemoved { at, lines: removed } => {
            let at = at.min(lines.len());
            for (k, text) in removed.into_iter().enumerate() {
                lines.insert(at + k, text);
            }
        }
        EditOp::Reorder { inverse, data_start } => {
            apply_permutation(lines, &inverse, data_start);
        }
        EditOp::Batch(ops) => {
            for inner in ops {
                apply_undo_op(lines, inner);
            }
        }
    }
}

/// 재배치 order의 역순열을 만든다. `apply_permutation(lines, order, ds)`를
/// 되돌리려면 `apply_permutation(lines, inverse_of(order, ds), ds)`를 적용하면 된다.
/// order[i] = 재배치 후 i번째 자리에 올 원본 논리 행번호.
pub fn inverse_of(order: &[u32], data_start: usize) -> Vec<u32> {
    let mut inv = vec![0u32; order.len()];
    for (new_i, &orig) in order.iter().enumerate() {
        let orig_slot = (orig as usize).saturating_sub(data_start);
        if orig_slot < inv.len() {
            inv[orig_slot] = (data_start + new_i) as u32;
        }
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_lf_basic() {
        // "a\nb\nc\n" → ["a","b","c"], newline=Lf
        let src = crate::source::Source::from_bytes(b"a\nb\nc\n".to_vec());
        let buf = load_edit_buffer(&src, Encoding::Utf8);
        assert_eq!(buf.lines, vec!["a", "b", "c"]);
        assert_eq!(buf.newline, Newline::Lf);
        assert!(!buf.dirty);
    }

    #[test]
    fn load_crlf_detected() {
        let src = crate::source::Source::from_bytes(b"a\r\nb\r\n".to_vec());
        let buf = load_edit_buffer(&src, Encoding::Utf8);
        assert_eq!(buf.lines, vec!["a", "b"]);
        assert_eq!(buf.newline, Newline::CrLf);
    }

    #[test]
    fn load_no_trailing_newline() {
        // 마지막 줄에 개행이 없으면 그 줄도 포함.
        let src = crate::source::Source::from_bytes(b"a\nb".to_vec());
        let buf = load_edit_buffer(&src, Encoding::Utf8);
        assert_eq!(buf.lines, vec!["a", "b"]);
    }

    #[test]
    fn load_empty_file_is_one_empty_line() {
        let src = crate::source::Source::from_bytes(b"".to_vec());
        let buf = load_edit_buffer(&src, Encoding::Utf8);
        assert_eq!(buf.lines, vec![""]);
        assert_eq!(buf.newline, Newline::Lf);
    }

    #[test]
    fn load_cp949_decodes() {
        // "가나" in CP949 = B0 A1 B3 AA, 한 줄.
        let src = crate::source::Source::from_bytes(vec![0xB0, 0xA1, 0xB3, 0xAA, b'\n']);
        let buf = load_edit_buffer(&src, Encoding::Cp949);
        assert_eq!(buf.lines, vec!["가나"]);
    }

    fn v(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn insert_char_mid_line() {
        let mut lines = v(&["abc"]);
        let p = insert_char(&mut lines, TextPos { line: 0, col: 1 }, 'X');
        assert_eq!(lines, v(&["aXbc"]));
        assert_eq!(p, TextPos { line: 0, col: 2 });
    }

    #[test]
    fn split_line_enter() {
        let mut lines = v(&["abcd"]);
        let p = split_line(&mut lines, TextPos { line: 0, col: 2 });
        assert_eq!(lines, v(&["ab", "cd"]));
        assert_eq!(p, TextPos { line: 1, col: 0 });
    }

    #[test]
    fn backspace_mid_line() {
        let mut lines = v(&["abc"]);
        let p = backspace(&mut lines, TextPos { line: 0, col: 2 });
        assert_eq!(lines, v(&["ac"]));
        assert_eq!(p, TextPos { line: 0, col: 1 });
    }

    #[test]
    fn backspace_at_line_start_merges() {
        // 줄 맨 앞 Backspace → 앞 줄과 병합, 커서는 병합점.
        let mut lines = v(&["ab", "cd"]);
        let p = backspace(&mut lines, TextPos { line: 1, col: 0 });
        assert_eq!(lines, v(&["abcd"]));
        assert_eq!(p, TextPos { line: 0, col: 2 });
    }

    #[test]
    fn backspace_at_origin_noop() {
        let mut lines = v(&["ab"]);
        let p = backspace(&mut lines, TextPos { line: 0, col: 0 });
        assert_eq!(lines, v(&["ab"]));
        assert_eq!(p, TextPos { line: 0, col: 0 });
    }

    #[test]
    fn delete_range_within_line() {
        let mut lines = v(&["abcdef"]);
        let p = delete_range(
            &mut lines,
            TextPos { line: 0, col: 1 },
            TextPos { line: 0, col: 4 },
        );
        assert_eq!(lines, v(&["aef"]));
        assert_eq!(p, TextPos { line: 0, col: 1 });
    }

    #[test]
    fn delete_range_multiline() {
        // 1행 col1 ~ 3행 col2 삭제 → 시작 줄 앞부분 + 끝 줄 뒷부분 병합.
        let mut lines = v(&["abc", "XXX", "defg"]);
        let p = delete_range(
            &mut lines,
            TextPos { line: 0, col: 1 },
            TextPos { line: 2, col: 2 },
        );
        // "a" + "fg" = "afg"
        assert_eq!(lines, v(&["afg"]));
        assert_eq!(p, TextPos { line: 0, col: 1 });
    }

    #[test]
    fn insert_str_with_newlines() {
        let mut lines = v(&["ad"]);
        let p = insert_str(&mut lines, TextPos { line: 0, col: 1 }, "b\nc");
        // "a" + "b" / "c" + "d" → ["ab","cd"]
        assert_eq!(lines, v(&["ab", "cd"]));
        assert_eq!(p, TextPos { line: 1, col: 1 });
    }

    #[test]
    fn selection_text_single_line() {
        let lines = v(&["abcdef"]);
        let s = selection_text(
            &lines,
            TextPos { line: 0, col: 1 },
            TextPos { line: 0, col: 4 },
        );
        assert_eq!(s, "bcd");
    }

    #[test]
    fn selection_text_multiline() {
        // delete_range_multiline과 같은 범위 — 지워지는 부분이 그대로 나와야 한다.
        let lines = v(&["abc", "XXX", "defg"]);
        let s = selection_text(
            &lines,
            TextPos { line: 0, col: 1 },
            TextPos { line: 2, col: 2 },
        );
        assert_eq!(s, "bc\nXXX\nde");
    }

    #[test]
    fn selection_text_empty_range() {
        let lines = v(&["abc"]);
        let s = selection_text(
            &lines,
            TextPos { line: 0, col: 2 },
            TextPos { line: 0, col: 2 },
        );
        assert_eq!(s, "");
    }

    // ---- word_range_at (더블클릭 선택) ----

    /// 범위를 눈으로 확인하기 쉽게 잘라서 돌려주는 보조.
    fn word_at(line: &str, col: usize) -> String {
        let (a, b) = word_range_at(line, col);
        line.chars().skip(a).take(b - a).collect()
    }

    #[test]
    fn word_range_grabs_space_delimited_chunk() {
        assert_eq!(word_at("hello world foo", 7), "world");
    }

    #[test]
    fn word_range_stops_at_tabs() {
        // TSV 한 줄: 탭이 경계다.
        assert_eq!(word_at("aa\tbbb\tcc", 4), "bbb");
    }

    #[test]
    fn word_range_keeps_punctuation_inside_the_chunk() {
        // 공백만 경계이므로 날짜나 점 찍힌 이름이 통째로 잡힌다.
        assert_eq!(word_at("x 2026-08-27 y", 5), "2026-08-27");
        assert_eq!(word_at("a.b.c d", 2), "a.b.c");
    }

    #[test]
    fn word_range_at_line_start_and_end() {
        assert_eq!(word_at("abc def", 0), "abc");
        // 줄 끝(= char 개수)에서 눌러도 마지막 덩어리를 잡는다.
        assert_eq!(word_at("abc def", 7), "def");
    }

    #[test]
    fn word_range_on_whitespace_selects_the_whitespace_run() {
        // 빈 선택으로 무너지지 않는다.
        assert_eq!(word_at("ab   cd", 3), "   ");
    }

    #[test]
    fn word_range_on_empty_line_is_empty() {
        assert_eq!(word_range_at("", 0), (0, 0));
    }

    #[test]
    fn word_range_whole_line_when_no_whitespace() {
        assert_eq!(word_at("abcdef", 3), "abcdef");
    }

    #[test]
    fn word_range_counts_chars_not_bytes() {
        // 한글은 UTF-8 3바이트다. col 은 char 인덱스여야 한다.
        assert_eq!(word_at("가나 다라마 바", 4), "다라마");
        let (a, b) = word_range_at("가나 다라마 바", 4);
        assert_eq!((a, b), (3, 6));
    }

    #[test]
    fn selection_text_reversed_is_normalized() {
        let lines = v(&["ab", "cd"]);
        let s = selection_text(
            &lines,
            TextPos { line: 1, col: 1 },
            TextPos { line: 0, col: 1 },
        );
        assert_eq!(s, "b\nc");
    }

    #[test]
    fn selection_text_two_adjacent_lines() {
        let lines = v(&["ab", "cd", "ef"]);
        let s = selection_text(
            &lines,
            TextPos { line: 0, col: 2 },
            TextPos { line: 1, col: 0 },
        );
        // 시작 줄 끝 ~ 다음 줄 처음 = 개행 하나.
        assert_eq!(s, "\n");
    }

    #[test]
    fn selection_text_multibyte_chars() {
        // col은 char 인덱스 — 바이트 인덱스로 자르면 깨진다.
        let lines = v(&["가나다라"]);
        let s = selection_text(
            &lines,
            TextPos { line: 0, col: 1 },
            TextPos { line: 0, col: 3 },
        );
        assert_eq!(s, "나다");
    }

    #[test]
    fn selection_text_mirrors_delete_range() {
        // selection_text가 뽑아낸 것을 다시 넣으면 원래대로 — 잘라내기/붙여넣기 왕복.
        let orig = v(&["hello", "world", "again"]);
        let a = TextPos { line: 0, col: 2 };
        let b = TextPos { line: 2, col: 3 };
        let cut = selection_text(&orig, a, b);
        let mut lines = orig.clone();
        let p = delete_range(&mut lines, a, b);
        let end = insert_str(&mut lines, p, &cut);
        assert_eq!(lines, orig);
        assert_eq!(end, b);
    }

    #[test]
    fn normalize_swaps_when_reversed() {
        let a = TextPos { line: 2, col: 0 };
        let b = TextPos { line: 1, col: 3 };
        assert_eq!(normalize(a, b), (b, a));
    }

    #[test]
    fn set_cell_replaces_field() {
        let mut lines = v(&["a,b,c"]);
        set_cell(&mut lines, 0, 1, "Z", b',');
        assert_eq!(lines, v(&["a,Z,c"]));
    }

    #[test]
    fn set_cell_pads_missing_columns() {
        // col 3을 설정하는데 필드가 2개뿐 → 빈 필드로 패딩.
        let mut lines = v(&["a,b"]);
        set_cell(&mut lines, 0, 3, "Z", b',');
        assert_eq!(lines, v(&["a,b,,Z"]));
    }

    #[test]
    fn set_cell_quotes_when_value_has_delim() {
        let mut lines = v(&["a,b"]);
        set_cell(&mut lines, 0, 0, "x,y", b',');
        assert_eq!(lines, v(&["\"x,y\",b"]));
    }

    #[test]
    fn set_cell_strips_newlines_from_value() {
        // 셀 값에 개행이 있으면 공백으로 치환 — lines[i]에 개행이 박히지 않아야 한다.
        let mut lines = v(&["a,b"]);
        set_cell(&mut lines, 0, 0, "x\ny", b',');
        assert_eq!(lines, v(&["x y,b"]));
        assert!(!lines[0].contains('\n'));
    }

    #[test]
    fn set_cell_strips_carriage_return() {
        let mut lines = v(&["a,b"]);
        set_cell(&mut lines, 0, 1, "p\r\nq", b',');
        // \r\n 두 문자가 각각 공백으로 → "p  q"
        assert_eq!(lines, v(&["a,p  q"]));
    }

    #[test]
    fn clear_cells_rectangle() {
        // 2x2 영역(행0~1, 열0~1)을 빈 값으로.
        let mut lines = v(&["a,b,c", "d,e,f", "g,h,i"]);
        clear_cells(&mut lines, 0, 0, 1, 1, b',');
        assert_eq!(lines, v(&[",,c", ",,f", "g,h,i"]));
    }

    /// 이미 비어 있는 대상 행은 재조립을 건너뛴다 — 그 결과 줄이 **바이트
    /// 단위로 그대로** 남아야 한다. app.rs의 Cut/Clear no-op undo 판정
    /// (`edit_op_differs_from_current`)이 "안 바뀜"을 정확히 보려면 이 성질이
    /// 필요하다. 특히 재조립(split+join)은 인용을 정규화해 버려서 값이
    /// 같더라도 줄 텍스트가 달라질 수 있다.
    #[test]
    fn clear_cells_skips_already_empty_rows_byte_exactly() {
        // col 1이 이미 비어 있고, col 0은 재조립되면 표현이 달라질 수 있는 값.
        let mut lines = v(&["\"a\"b,,c", ",,", "x,y,z"]);
        let before = lines.clone();
        clear_cells(&mut lines, 0, 1, 1, 1, b',');
        assert_eq!(lines[0], before[0], "이미 빈 행은 손대지 않는다");
        assert_eq!(lines[1], before[1], "이미 빈 행은 손대지 않는다");
        assert_eq!(lines[2], before[2], "범위 밖 행은 그대로");
    }

    #[test]
    fn clear_cells_still_clears_non_empty() {
        // 건너뛰기 최적화가 실제로 지워야 할 행까지 건너뛰면 안 된다.
        let mut lines = v(&["a,b,c", "d,,f", "g,h,i"]);
        clear_cells(&mut lines, 0, 1, 2, 1, b',');
        assert_eq!(lines, v(&["a,,c", "d,,f", "g,,i"]));
    }

    #[test]
    fn insert_and_remove_row() {
        let mut lines = v(&["a", "b"]);
        insert_row(&mut lines, 1, String::new());
        assert_eq!(lines, v(&["a", "", "b"]));
        remove_row(&mut lines, 1);
        assert_eq!(lines, v(&["a", "b"]));
    }

    #[test]
    fn remove_last_row_keeps_one_empty() {
        // 마지막 한 줄을 지우면 빈 한 줄은 남긴다(빈 lines 방지).
        let mut lines = v(&["only"]);
        remove_row(&mut lines, 0);
        assert_eq!(lines, v(&[""]));
    }

    #[test]
    fn cells_to_tsv_basic() {
        let lines = v(&["a,b,c", "d,e,f"]);
        // 열 0~1, 행 0~1 → "a\tb\nd\te"
        let s = cells_to_tsv(&lines, 0, 0, 1, 1, b',');
        assert_eq!(s, "a\tb\nd\te");
    }

    #[test]
    fn cells_to_tsv_single_column_matches_split_fields() {
        // 최적화 경로(단일 컬럼)와 일반 경로가 같은 결과를 내야 한다.
        let lines = v(&["a,b,c", "d,e,f", "\"x,y\",z,w"]);
        for col in 0..3 {
            let fast = cells_to_tsv(&lines, 0, col, 2, col, b',');
            // 일반 경로를 직접 재현(참조 구현).
            let mut want = String::new();
            for (i, l) in lines.iter().enumerate() {
                if i > 0 {
                    want.push('\n');
                }
                let f = crate::parse::split_fields(l, b',');
                want.push_str(f.get(col).map(|s| s.as_str()).unwrap_or(""));
            }
            assert_eq!(fast, want, "col {col}");
        }
    }

    /// `unquote_field`가 `split_fields`의 인용 해제 의미를 정확히 재현하는지.
    /// 감싸는 따옴표 제거 + 내부 `""` → `"` 복원, 인용이 아닌 값은 그대로.
    #[test]
    fn cells_to_tsv_matches_split_fields_on_tricky_quoting() {
        let lines = v(&[
            "\"a\"\"b\",z",       // 내부 이스케이프된 따옴표
            "\"\",z",             // 빈 인용 필드
            ",z",                 // 빈 필드(인용 아님)
            "\"only\",z",         // 평범한 인용
            "\"\"\"\",z",         // 인용 안의 따옴표 하나
        ]);
        for col in 0..2 {
            let fast = cells_to_tsv(&lines, 0, col, lines.len() - 1, col, b',');
            let mut want = String::new();
            for (i, l) in lines.iter().enumerate() {
                if i > 0 {
                    want.push('\n');
                }
                let f = crate::parse::split_fields(l, b',');
                want.push_str(f.get(col).map(|s| s.as_str()).unwrap_or(""));
            }
            assert_eq!(fast, want, "col {col}");
        }
    }

    /// `{a, ", ,}` 알파벳으로 만든 길이 0~5의 **모든** 문자열에 대해
    /// `cells_to_tsv`(field_slice + 인용 해제)가 `split_fields` 경로와
    /// 완전히 같은 값을 내는지 전수 검사한다. 브리프의 단순 인용 해제
    /// 규칙이 어긋났던 케이스(`"`, `"ab`, `"a"b"` 등)가 모두 여기 포함된다.
    #[test]
    fn cells_to_tsv_matches_split_fields_exhaustively() {
        const ALPHABET: [char; 3] = ['a', '"', ','];
        let mut checked = 0usize;
        for len in 0..=5usize {
            let total = ALPHABET.len().pow(len as u32);
            for n in 0..total {
                let mut n = n;
                let mut s = String::new();
                for _ in 0..len {
                    s.push(ALPHABET[n % ALPHABET.len()]);
                    n /= ALPHABET.len();
                }
                let lines = vec![s.clone()];
                let want = crate::parse::split_fields(&s, b',');
                for col in 0..want.len() {
                    let got = cells_to_tsv(&lines, 0, col, 0, col, b',');
                    assert_eq!(got, want[col], "input {s:?} col {col}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 500, "전수 검사가 실제로 돌았는지 확인");
    }

    /// 대용량 컬럼 복사 벤치. 무시 상태이며 필요할 때만 돌린다:
    ///   cargo test --release bench_column_copy -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_column_copy() {
        use std::time::Instant;
        let rows = 2_000_000;
        let lines: Vec<String> = (0..rows).map(|i| format!("{i},bbb,ccc,ddd")).collect();
        let t = Instant::now();
        let s = cells_to_tsv(&lines, 0, 1, rows - 1, 1, b',');
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!("컬럼 복사 {rows}행: {ms:.0}ms, 결과 {}바이트", s.len());
    }

    /// 대용량 컬럼 지우기 벤치(clear_cells). 이미 비어 있는 행은 재조립을
    /// 건너뛴다는 것을 확인하기 위해 "이미 빈 컬럼" 케이스를 함께 잰다.
    ///   cargo test --release bench_column_clear -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_column_clear() {
        use std::time::Instant;
        let rows = 2_000_000;
        let mut lines: Vec<String> = (0..rows).map(|i| format!("{i},bbb,ccc,ddd")).collect();
        let t = Instant::now();
        clear_cells(&mut lines, 0, 1, rows - 1, 1, b',');
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!("컬럼 지우기(값 있음) {rows}행: {ms:.0}ms");
        // 두 번째 호출: 이미 전부 비어 있으므로 재조립을 건너뛰어야 한다.
        let t = Instant::now();
        clear_cells(&mut lines, 0, 1, rows - 1, 1, b',');
        let ms2 = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!("컬럼 지우기(이미 빔) {rows}행: {ms2:.0}ms");
    }

    #[test]
    fn paste_tsv_overwrites_grid() {
        let mut lines = v(&["a,b,c", "d,e,f"]);
        // (0,1)부터 "X\tY\nZ\tW" 붙여넣기 → 행0 col1,2 = X,Y / 행1 col1,2 = Z,W
        paste_tsv(&mut lines, 0, 1, "X\tY\nZ\tW", b',');
        assert_eq!(lines, v(&["a,X,Y", "d,Z,W"]));
    }

    #[test]
    fn paste_tsv_extends_rows_and_cols() {
        // 파일 경계를 넘는 붙여넣기 → 행/열 확장.
        let mut lines = v(&["a"]);
        paste_tsv(&mut lines, 0, 0, "1\t2\n3\t4", b',');
        assert_eq!(lines, v(&["1,2", "3,4"]));
    }

    #[test]
    fn paste_tsv_sanitizes_carriage_return() {
        // CRLF 클립보드: \n으로 split 후 남는 \r이 셀에 박히면 안 된다.
        let mut lines = v(&["a,b"]);
        paste_tsv(&mut lines, 0, 0, "X\r\nY", b',');
        assert!(!lines.iter().any(|l| l.contains('\r')));
        assert!(!lines.iter().any(|l| l.contains('\n')));
    }

    #[test]
    fn apply_permutation_reorders_data_rows() {
        // order = [2,1] (논리 행번호), data_start=1 → 헤더 유지, 데이터 재배치.
        let mut lines = v(&["name", "banana", "apple"]);
        apply_permutation(&mut lines, &[2, 1], 1);
        assert_eq!(lines, v(&["name", "apple", "banana"]));
    }

    #[test]
    fn apply_permutation_no_header() {
        let mut lines = v(&["b", "a", "c"]);
        apply_permutation(&mut lines, &[1, 0, 2], 0);
        assert_eq!(lines, v(&["a", "b", "c"]));
    }

    /// `apply_permutation`이 clone 판과 **글자 그대로 같은** 결과를 내는지
    /// 전수 비교한다. move 판으로 바꾸면서 중복 인덱스·범위 밖 인덱스·헤더를
    /// 가리키는 인덱스가 새 위험으로 생겼는데(그 함수 주석 참조), 정상 순열만
    /// 넣어 보면 셋 다 안 걸린다. 그래서 순열이 **아닌** order까지 포함해
    /// 작은 크기에서 모든 조합을 돌린다.
    #[test]
    fn apply_permutation_move_matches_clone_exhaustively() {
        // 옛 구현 그대로(비교 기준).
        fn clone_based(lines: &mut Vec<String>, order: &[u32], data_start: usize) {
            if data_start > lines.len() {
                return;
            }
            let header: Vec<String> = lines[..data_start].to_vec();
            let mut reordered: Vec<String> = Vec::with_capacity(order.len());
            for &idx in order {
                let i = idx as usize;
                if i < lines.len() {
                    reordered.push(lines[i].clone());
                }
            }
            let mut out = header;
            out.append(&mut reordered);
            *lines = out;
        }
        let base = v(&["h", "a", "b", "c"]);
        // order 길이 0~3, 각 원소는 0..=4(4는 lines 범위 밖) → 순열이 아닌 조합도
        // 전부 돈다. 5진수 카운터를 그냥 펼쳐서 조합을 만든다.
        let mut cases = 0usize;
        for len in 0..=3usize {
            let total = 5usize.pow(len as u32);
            for n in 0..total {
                let order: Vec<u32> =
                    (0..len).map(|k| (n / 5usize.pow(k as u32) % 5) as u32).collect();
                for ds in 0..=2usize {
                    let mut got = base.clone();
                    let mut want = base.clone();
                    apply_permutation(&mut got, &order, ds);
                    clone_based(&mut want, &order, ds);
                    assert_eq!(got, want, "order={order:?} data_start={ds}");
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, (1 + 5 + 25 + 125) * 3);
    }

    /// revision은 편집마다 **달라져야** 한다 — 캐시 무효화가 이 값에만
    /// 기대기 때문이다.
    #[test]
    fn revision_advances_on_every_push() {
        let mut st = UndoStack::new(UNDO_LIMIT);
        let start = st.revision();
        for i in 0..5 {
            st.push(EditOp::Replace(vec![(i, "x".to_string())]));
        }
        assert_eq!(st.revision(), start + 5);
    }

    /// 되돌리기도 내용을 바꾸므로 revision이 올라간다. 되돌아가서 **같은 값**이
    /// 되면 "편집 → 되돌리기" 뒤의 캐시가 최신인 척 되살아난다.
    #[test]
    fn revision_advances_on_undo_too() {
        let mut lines = v(&["a", "b"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        st.push(EditOp::Replace(vec![(0, lines[0].clone())]));
        lines[0] = "X".to_string();
        let after_push = st.revision();
        assert!(st.undo(&mut lines));
        assert_ne!(st.revision(), after_push, "되돌리기도 편집이다");
    }

    /// 되돌릴 것이 없으면 내용도 그대로이므로 revision은 그대로여야 한다.
    #[test]
    fn revision_unchanged_when_undo_has_nothing() {
        let mut lines = v(&["a"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        let before = st.revision();
        assert!(!st.undo(&mut lines));
        assert_eq!(st.revision(), before);
    }

    /// 스택 상한에 걸려 op가 버려져도 revision은 올라간다. 스택 길이로
    /// 무효화를 판단하면 이 경우를 놓친다(길이가 그대로다).
    #[test]
    fn revision_advances_even_when_ops_are_evicted() {
        let mut st = UndoStack::new(2);
        for i in 0..6 {
            st.push(EditOp::Replace(vec![(i, "x".to_string())]));
        }
        assert_eq!(st.len(), 2, "스택 길이는 상한에 눌린다");
        assert_eq!(st.revision(), 6, "그래도 편집은 6번 일어났다");
    }

    /// 되돌리기를 아예 못 쌓는 설정(limit 0)에서도 편집은 일어난다.
    #[test]
    fn revision_advances_when_undo_disabled() {
        let mut st = UndoStack::new(0);
        st.push(EditOp::Replace(vec![(0, "x".to_string())]));
        assert_eq!(st.len(), 0);
        assert_eq!(st.revision(), 1);
    }

    #[test]
    fn undo_replace_restores_previous_lines() {
        let mut lines = v(&["a,b", "c,d"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        // 1행을 바꾸기 전에 이전 값을 기록.
        st.push(EditOp::Replace(vec![(1, lines[1].clone())]));
        lines[1] = "X,Y".to_string();
        assert!(st.undo(&mut lines));
        assert_eq!(lines, v(&["a,b", "c,d"]));
    }

    #[test]
    fn undo_remove_inserted_rows() {
        let mut lines = v(&["a", "b"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        insert_row(&mut lines, 1, String::new());
        st.push(EditOp::RemoveInserted { at: 1, count: 1 });
        assert_eq!(lines, v(&["a", "", "b"]));
        assert!(st.undo(&mut lines));
        assert_eq!(lines, v(&["a", "b"]));
    }

    #[test]
    fn undo_reinsert_removed_rows() {
        let mut lines = v(&["a", "b", "c"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        st.push(EditOp::ReinsertRemoved { at: 1, lines: vec!["b".to_string()] });
        remove_row(&mut lines, 1);
        assert_eq!(lines, v(&["a", "c"]));
        assert!(st.undo(&mut lines));
        assert_eq!(lines, v(&["a", "b", "c"]));
    }

    #[test]
    fn undo_reorder_restores_original_order() {
        // 정렬로 재배치한 뒤 undo하면 원래 순서로 돌아온다(헤더 유지).
        let mut lines = v(&["hdr", "b", "a", "c"]);
        let order: Vec<u32> = vec![2, 1, 3]; // a(2), b(1), c(3)
        let inverse = inverse_of(&order, 1);
        let mut st = UndoStack::new(UNDO_LIMIT);
        st.push(EditOp::Reorder { inverse, data_start: 1 });
        apply_permutation(&mut lines, &order, 1);
        assert_eq!(lines, v(&["hdr", "a", "b", "c"]));
        assert!(st.undo(&mut lines));
        assert_eq!(lines, v(&["hdr", "b", "a", "c"]));
    }

    #[test]
    fn undo_pops_in_lifo_order() {
        let mut lines = v(&["a"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        st.push(EditOp::Replace(vec![(0, "a".to_string())]));
        lines[0] = "b".to_string();
        st.push(EditOp::Replace(vec![(0, "b".to_string())]));
        lines[0] = "c".to_string();
        st.undo(&mut lines);
        assert_eq!(lines, v(&["b"]), "최근 것부터 되돌린다");
        st.undo(&mut lines);
        assert_eq!(lines, v(&["a"]));
    }

    #[test]
    fn undo_on_empty_stack_is_false() {
        let mut lines = v(&["a"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        assert!(!st.undo(&mut lines));
        assert_eq!(lines, v(&["a"]));
    }

    #[test]
    fn undo_stack_drops_oldest_beyond_limit() {
        let mut st = UndoStack::new(2);
        st.push(EditOp::Replace(vec![(0, "1".into())]));
        st.push(EditOp::Replace(vec![(0, "2".into())]));
        st.push(EditOp::Replace(vec![(0, "3".into())]));
        assert_eq!(st.len(), 2, "가장 오래된 것이 버려진다");
    }

    /// 예산 안이면 아무것도 안 버린다 — 바이트 예산이 단계 수 제한을
    /// 앞질러 작동하면 정상 편집에서도 되돌리기가 조용히 사라진다.
    #[test]
    fn undo_byte_budget_keeps_everything_when_under() {
        let mut st = UndoStack::with_budget(100, 1 << 20);
        for i in 0..10 {
            st.push(EditOp::Replace(vec![(i, "x".repeat(100))]));
        }
        assert_eq!(st.len(), 10);
        assert!(st.bytes() < 1 << 20);
    }

    /// 예산을 넘기면 오래된 단계부터 버린다(FIFO 유지).
    #[test]
    fn undo_byte_budget_drops_oldest_when_over() {
        // op 하나가 1,024바이트 + 고정비 32바이트. 예산을 3개치로 잡는다.
        let one = 1024 + 32;
        let mut st = UndoStack::with_budget(100, one * 3);
        for i in 0..6 {
            st.push(EditOp::Replace(vec![(i, "x".repeat(1024))]));
        }
        assert_eq!(st.len(), 3, "예산이 3개치면 최근 3개만 남는다");
        assert!(st.bytes() <= one * 3);
        // 남은 3개가 **최근** 3개(3,4,5행)인지 하나씩 되돌려 확인한다.
        // 개수만 세면 "오래된 것 대신 최신 것을 버리는" 뒤집힌 구현도 통과한다 —
        // 각 단계가 어느 행을 복원하는지까지 봐야 FIFO가 진짜로 지켜진다.
        let mut lines = v(&["", "", "", "", "", ""]);
        let mut restored = Vec::new();
        while st.undo(&mut lines) {
            let r = lines.iter().position(|l| !l.is_empty()).expect("복원된 행이 있어야 한다");
            restored.push(r);
            lines[r].clear();
        }
        assert_eq!(restored, vec![5, 4, 3], "최근 3개가 LIFO 순서로 남아 있어야 한다");
    }

    /// 단계 수 제한은 예산과 **무관하게** 여전히 작동한다(둘 중 먼저 걸리는
    /// 쪽이 이긴다). 예산을 사실상 무한으로 두고 확인한다.
    #[test]
    fn undo_step_limit_still_applies_with_huge_budget() {
        let mut st = UndoStack::with_budget(2, usize::MAX);
        for i in 0..5 {
            st.push(EditOp::Replace(vec![(i, "x".into())]));
        }
        assert_eq!(st.len(), 2);
    }

    /// 새 op 하나가 혼자 예산을 넘겨도 그 하나는 남는다 — 되돌리기 자체를
    /// 포기시키면 큰 파일에서 전체 바꾸기를 못 되돌린다.
    #[test]
    fn undo_oversized_single_op_is_still_kept() {
        let mut st = UndoStack::with_budget(100, 64);
        st.push(EditOp::Replace(vec![(0, "x".repeat(10))]));
        st.push(EditOp::Replace(vec![(1, "y".repeat(10_000))]));
        assert_eq!(st.len(), 1, "예산 초과라도 최신 하나는 남는다");
        assert!(st.bytes() > 64);
    }

    /// 되돌린 단계는 회계에서도 빠진다 — 안 빼면 undo를 반복할수록 남은
    /// 예산을 실제보다 작게 보아 멀쩡한 단계를 버리기 시작한다.
    #[test]
    fn undo_pop_releases_its_bytes() {
        let mut st = UndoStack::with_budget(100, 1 << 20);
        st.push(EditOp::Replace(vec![(0, "x".repeat(1000))]));
        let after_push = st.bytes();
        assert!(after_push >= 1000);
        let mut lines = v(&["a"]);
        assert!(st.undo(&mut lines));
        assert_eq!(st.bytes(), 0, "되돌린 단계의 바이트는 반납된다");
    }

    /// `undo_evict_count`는 순수 함수라 스택을 꾸미지 않고 규칙만 직접 찌른다.
    #[test]
    fn undo_evict_count_rules() {
        // 예산·단계 수 둘 다 여유 → 아무것도 안 버린다.
        assert_eq!(undo_evict_count(&[10, 10], 10, 100, 1000), 0);
        // 단계 수가 꽉 참 → 자리 하나를 만든다.
        assert_eq!(undo_evict_count(&[10, 10], 10, 2, 1000), 1);
        // 단계 수는 여유지만 예산 초과 → 넘지 않을 때까지 앞에서 버린다.
        assert_eq!(undo_evict_count(&[10, 10, 10], 10, 100, 30), 1);
        assert_eq!(undo_evict_count(&[10, 10, 10], 25, 100, 30), 3);
        // 새 op 혼자 예산 초과 → 전부 버리되 개수는 기존 길이를 안 넘는다.
        assert_eq!(undo_evict_count(&[10, 10], 9999, 100, 30), 2);
        // 빈 스택.
        assert_eq!(undo_evict_count(&[], 9999, 100, 30), 0);
        // 단계 수 초과분이 2 이상인 경우(제한이 줄어든 상황)도 한 번에 정리.
        assert_eq!(undo_evict_count(&[1, 1, 1, 1], 1, 2, 1000), 3);
    }

    /// `op_bytes`는 정확할 필요는 없지만 **과소평가하면 안 된다** — 내용이
    /// 짧고 원소가 많은 op가 0으로 계산되면 예산이 통째로 무력해진다.
    #[test]
    fn op_bytes_counts_per_element_overhead() {
        // 빈 문자열 1,000개라도 포인터·길이 몫은 잡힌다.
        let many_empty = EditOp::Replace((0..1000).map(|i| (i, String::new())).collect());
        assert!(op_bytes(&many_empty) >= 1000 * 8, "원소당 고정 비용을 세야 한다");
        // 내용도 함께 잡힌다.
        let with_text = EditOp::Replace(vec![(0, "x".repeat(5000))]);
        assert!(op_bytes(&with_text) >= 5000);
        // Batch는 안쪽 합.
        let batch = EditOp::Batch(vec![
            EditOp::Replace(vec![(0, "x".repeat(100))]),
            EditOp::ReinsertRemoved { at: 0, lines: vec!["y".repeat(100)] },
        ]);
        assert!(op_bytes(&batch) >= 200);
        // Reorder는 인덱스 배열 크기.
        let reorder = EditOp::Reorder { inverse: vec![0u32; 1000], data_start: 0 };
        assert_eq!(op_bytes(&reorder), 4000);
    }

    #[test]
    fn undo_replace_out_of_range_row_is_ignored() {
        // 행이 줄어든 뒤 낡은 인덱스가 들어와도 패닉하지 않는다.
        let mut lines = v(&["a"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        st.push(EditOp::Replace(vec![(5, "x".to_string())]));
        assert!(st.undo(&mut lines));
        assert_eq!(lines, v(&["a"]));
    }

    #[test]
    fn undo_batch_is_one_step() {
        // 붙여넣기로 행이 늘어난 상황: 덮어쓴 행 복원 + 늘어난 행 제거가
        // Ctrl+Z 한 번에 모두 일어나야 한다.
        let mut lines = v(&["a,b"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        st.push(EditOp::Batch(vec![
            EditOp::RemoveInserted { at: 1, count: 1 },
            EditOp::Replace(vec![(0, "a,b".to_string())]),
        ]));
        paste_tsv(&mut lines, 0, 0, "1\t2\n3\t4", b',');
        assert_eq!(lines, v(&["1,2", "3,4"]));
        assert!(st.undo(&mut lines));
        assert_eq!(lines, v(&["a,b"]), "한 번의 undo로 완전 복구");
        assert!(st.is_empty(), "Batch는 한 단계만 소비한다");
    }

    #[test]
    fn undo_batch_applies_in_order() {
        // 담긴 순서대로 적용된다: 먼저 유령 줄 제거 → 그다음 원본 되꽂기.
        let mut lines = v(&["only"]);
        let mut st = UndoStack::new(UNDO_LIMIT);
        st.push(EditOp::Batch(vec![
            EditOp::RemoveInserted { at: 0, count: 1 },
            EditOp::ReinsertRemoved { at: 0, lines: v(&["x", "y"]) },
        ]));
        // 전부 삭제 → remove_row가 빈 한 줄을 남긴다.
        remove_row(&mut lines, 0);
        assert_eq!(lines, v(&[""]));
        assert!(st.undo(&mut lines));
        assert_eq!(lines, v(&["x", "y"]), "유령 줄 없이 정확히 복원");
    }

    #[test]
    fn inverse_of_roundtrips() {
        let order: Vec<u32> = vec![3, 1, 2];
        let inv = inverse_of(&order, 1);
        let mut lines = v(&["h", "x", "y", "z"]);
        let before = lines.clone();
        apply_permutation(&mut lines, &order, 1);
        apply_permutation(&mut lines, &inv, 1);
        assert_eq!(lines, before);
    }
}
