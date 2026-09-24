//! UI 문구를 한곳에 모은다. **문구를 고치거나 언어를 더할 곳은 여기뿐이다.**
//!
//! # 언어를 추가하려면
//!
//! 1. [`Lang`]에 갈래를 하나 넣는다.
//! 2. [`Lang::from_tag`]에 그 언어의 태그를 넣는다.
//! 3. `ui!` 안의 모든 항목에 그 언어 문구를 적는다.
//!
//! 3번을 빠뜨릴 수 없다 — 매크로가 항목마다 `match`를 펼치므로, 빠진
//! 문구가 있으면 **컴파일이 실패하고 어느 항목인지 이름으로 알려준다.**
//! 언어별로 파일을 나누면 이 검사가 사라져(한쪽에만 있는 키를 아무도 못
//! 잡는다) 일부러 키 하나에 모든 언어를 나란히 두었다.
//!
//! # 쓰는 법
//!
//! ```ignore
//! ui.button(t(self.lang).menu_open);          // 고정 문구
//! ui.label(format!("{} {}", t(l).sort_priority, i + 1));   // 값이 끼는 문구
//! ```

/// 지원 언어. 기본값은 [`Lang::detect`]가 OS에서 정한다.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    Ko,
    #[default]
    En,
}

impl Lang {
    /// BCP-47 태그(`ko-KR`, `en-US`, `ko` …)에서 언어를 고른다.
    ///
    /// 지역(`-KR`)은 보지 않는다 — 한국어는 지역이 하나뿐이라 구분할
    /// 이득이 없다. 모르는 태그는 영어로 떨어진다.
    pub fn from_tag(tag: &str) -> Self {
        let primary = tag
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match primary.as_str() {
            "ko" => Lang::Ko,
            _ => Lang::En,
        }
    }

    /// OS 로케일에서 언어를 정한다. 못 읽으면 영어.
    pub fn detect() -> Self {
        sys_locale::get_locale()
            .map(|t| Lang::from_tag(&t))
            .unwrap_or(Lang::En)
    }

    /// 언어 선택 메뉴에 보일 이름. 각 언어를 **그 언어로** 적는다 —
    /// 영어 UI를 보고 있어도 "한국어"를 찾을 수 있어야 한다.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Ko => "한국어",
            Lang::En => "English",
        }
    }

    /// 메뉴에 순서대로 늘어놓기 위한 목록.
    pub const ALL: &'static [Lang] = &[Lang::En, Lang::Ko];
}

/// 언어별 문구 묶음을 정의한다. 자세한 것은 이 모듈의 문서를 보라.
macro_rules! ui {
    ($($key:ident { ko: $ko:expr, en: $en:expr })*) => {
        /// 한 언어의 전체 문구. `t(lang)`이 돌려준다.
        #[allow(non_snake_case)]
        pub struct Strings {
            $(pub $key: &'static str,)*
        }

        const KO: Strings = Strings { $($key: $ko,)* };
        const EN: Strings = Strings { $($key: $en,)* };

        /// 그 언어의 문구 묶음.
        pub fn t(lang: Lang) -> &'static Strings {
            match lang {
                Lang::Ko => &KO,
                Lang::En => &EN,
            }
        }
    };
}

ui! {
    // ---- 메뉴 막대 ----
    menu_file            { ko: "파일",              en: "File" }
    menu_edit            { ko: "편집",              en: "Edit" }
    menu_tools           { ko: "도구",              en: "Tools" }
    menu_language        { ko: "언어",              en: "Language" }
    menu_new             { ko: "새 파일",           en: "New" }
    menu_open            { ko: "열기…",             en: "Open…" }
    menu_save            { ko: "저장",              en: "Save" }
    menu_edit_mode       { ko: "편집 모드",         en: "Edit Mode" }
    menu_undo            { ko: "되돌리기   Ctrl+Z", en: "Undo   Ctrl+Z" }
    menu_find_replace    { ko: "찾기·바꾸기…   Ctrl+F", en: "Find / Replace…   Ctrl+F" }
    menu_sort_columns    { ko: "열 기준 정렬…",     en: "Sort by Columns…" }
    menu_convert_delim   { ko: "구분자 변환…",      en: "Convert Delimiter…" }
    menu_bad_rows        { ko: "오류 행…",          en: "Bad Rows…" }
    menu_row_col_numbers { ko: "행·열 번호…",       en: "Row & Column Numbers…" }

    // ---- 찾기·바꾸기 ----
    find_label           { ko: "찾기:",             en: "Find:" }
    find_replace_label   { ko: "바꾸기:",           en: "Replace:" }
    find_hex             { ko: "헥스",              en: "Hex" }
    find_next            { ko: "다음 찾기",         en: "Find Next" }
    find_prev            { ko: "이전 찾기",         en: "Find Prev" }
    find_all             { ko: "모두 찾기",         en: "Find All" }
    find_replace_one     { ko: "바꾸기",            en: "Replace" }
    find_replace_all     { ko: "모두 바꾸기",       en: "Replace All" }
    find_extract_rows    { ko: "행 추출",           en: "Extract Rows" }
    find_partial         { ko: "부분 일치",         en: "Partial" }
    find_whole_word      { ko: "단어 단위",         en: "Whole word" }
    find_whole_cell      { ko: "셀 전체",           en: "Whole cell" }
    find_match_case      { ko: "대소문자 구분",     en: "Match case" }
    find_escapes         { ko: "이스케이프 문자",   en: "Escape sequences" }

    // ---- 저장 ----
    save_overwrite       { ko: "덮어쓰기:",         en: "Overwrite:" }
    save_include_bom     { ko: "BOM 포함",          en: "Include BOM" }
    save_cp949_no_bom    { ko: "(CP949는 BOM이 없습니다)", en: "(CP949 has no BOM)" }

    // ---- 파일 열기(인코딩·바이너리) ----
    open_as_binary       { ko: "바이너리(헥스)로 열기", en: "Open as Binary (Hex)" }
    open_force_encoding  { ko: "또는 인코딩을 지정해 열기:", en: "Or force a text encoding:" }
    open_as_text         { ko: "텍스트로 열기",     en: "Open as Text" }

    // ---- 구분자 변환 ----
    convert_to           { ko: "변환할 구분자",     en: "Convert to" }
    convert_do           { ko: "변환",              en: "Convert" }
    convert_custom       { ko: "직접 입력:",        en: "Custom:" }
    convert_warn_change  { ko: "파일 내용이 바뀝니다. 되돌리려면 Ctrl+Z.",
                           en: "This changes the file contents. Press Ctrl+Z to undo." }
    convert_warn_save    { ko: "저장해야 디스크에 반영됩니다. 뷰 모드면 편집 모드로 전환됩니다.",
                           en: "Changes reach the disk only when you save. View mode switches to edit mode." }

    // ---- 오류 행 ----
    bad_checking         { ko: "행 검사 중…",       en: "Checking rows…" }
    bad_not_checked      { ko: "아직 검사하지 않았습니다.", en: "Not checked yet." }
    bad_none             { ko: "오류 행이 없습니다.", en: "No bad rows." }
    bad_click_to_jump    { ko: "이 행으로 이동",    en: "Click to jump to this row" }
    bad_click_to_list    { ko: "목록 보기",         en: "Click to see the list" }

    // ---- 정렬 ----
    sort_do              { ko: "정렬",              en: "Sort" }
    sort_running         { ko: "정렬 중…",          en: "Sorting…" }
    sort_clear           { ko: "정렬 해제",         en: "Clear Sort" }
    sort_priority        { ko: "우선순위",          en: "Priority" }
    sort_ignore_case     { ko: "대소문자 무시",     en: "Ignore case" }
    sort_add_criterion   { ko: "+ 기준 추가",       en: "+ Add criterion" }
    sort_all_in_use      { ko: "(모든 열을 쓰고 있습니다)", en: "(all columns in use)" }
    sort_maximum         { ko: "(최대 {})",         en: "(maximum {})" }
    sort_column_n        { ko: "정렬: {}번째 열",   en: "Sort: column {}" }
    sort_pick_header     { ko: "정렬: (헤더를 눌러 열을 고르세요)",
                           en: "Sort: (click a header to select a column)" }
    sort_text_asc        { ko: "문자 ↑",            en: "Text ↑" }
    sort_text_desc       { ko: "문자 ↓",            en: "Text ↓" }
    sort_number_asc      { ko: "숫자 ↑",            en: "Number ↑" }
    sort_number_desc     { ko: "숫자 ↓",            en: "Number ↓" }

    // ---- 공통 ----
    // ---- 컬럼 필터 드롭다운 ----
    filter_title         { ko: "필터 — {}",          en: "Filter — {}" }
    filter_column_n      { ko: "{}번째 열",          en: "Column {}" }
    filter_search        { ko: "검색 / 포함:",       en: "Search / contains:" }
    filter_number_range  { ko: "숫자 범위:",         en: "Number range:" }
    filter_invalid_number{ ko: "(숫자가 아님)",      en: "(not a number)" }
    filter_distinct_n    { ko: "고유값 {}개",         en: "{} distinct values" }
    filter_distinct_est  { ko: "고유값 약 {}개(추정)", en: "~{} distinct values (estimated)" }
    filter_truncated     { ko: "고유값이 너무 많아 앞 {}개만 보입니다. 위 검색으로 좁히세요.",
                           en: "Too many distinct values — showing first {}. Use the text filter above to narrow down." }
    filter_select_all    { ko: "전체 선택",          en: "Select All" }
    filter_clear_all     { ko: "전체 해제",          en: "Clear All" }
    filter_apply         { ko: "적용",              en: "Apply" }
    filter_clear         { ko: "필터 해제",          en: "Clear Filter" }
    filter_scanning      { ko: "스캔 중…",           en: "Scanning…" }
    filter_blank         { ko: "(빈 값)",            en: "(blank)" }

    common_cancel        { ko: "취소",              en: "Cancel" }
    common_continue      { ko: "계속",              en: "Continue" }
    common_close         { ko: "닫기",              en: "Close" }
    common_load          { ko: "불러오기",          en: "Load" }
    common_stop          { ko: "중지",              en: "Stop" }
    common_resume        { ko: "재개",              en: "Resume" }
    common_header        { ko: "헤더",              en: "Header" }
    common_rows          { ko: "행:",               en: "Rows:" }
    common_columns       { ko: "열:",               en: "Columns:" }
    common_line          { ko: "줄",                en: "Line" }
    common_no_file       { ko: "열린 파일 없음",    en: "No file open" }

    // ---- 오류 메시지 ----
    //
    // 사용자가 직접 부딪히는 것만 여기 둔다. `decode_group` 같은 내부 경로의
    // 오류는 영어로 고정한다 — 화면에 그대로 뜨는 문구가 아니라 상위가 감싸는
    // 배관이고, 언어를 태우려면 데이터 계층 전체에 `lang`을 흘려야 한다.
    err_open_file        { ko: "파일을 열 수 없습니다", en: "Cannot open file" }
    err_parquet_read     { ko: "Parquet으로 읽을 수 없습니다", en: "Cannot read as Parquet" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_maps_korean_variants_to_ko() {
        for t in ["ko", "ko-KR", "ko_KR", "KO-kr"] {
            assert_eq!(Lang::from_tag(t), Lang::Ko, "{t}");
        }
    }

    #[test]
    fn unknown_and_other_tags_fall_back_to_english() {
        for t in ["en", "en-US", "ja-JP", "de", "", "zz-ZZ"] {
            assert_eq!(Lang::from_tag(t), Lang::En, "{t}");
        }
    }

    /// 문구가 비어 있으면 UI에 빈 버튼이 생긴다 — 눈으로는 못 잡는다.
    #[test]
    fn no_string_is_empty_in_any_language() {
        for &l in Lang::ALL {
            let s = t(l);
            // 대표 항목만이 아니라 전부 봐야 의미가 있으므로, 구조체를
            // 필드별로 훑는 대신 눈에 띄는 것들을 표본으로 확인한다.
            for (name, v) in [
                ("menu_file", s.menu_file),
                ("menu_open", s.menu_open),
                ("find_next", s.find_next),
                ("sort_do", s.sort_do),
                ("common_cancel", s.common_cancel),
                ("err_open_file", s.err_open_file),
            ] {
                assert!(!v.trim().is_empty(), "{l:?}.{name} 이 비어 있다");
            }
        }
    }

    /// 값이 끼는 문구는 두 언어 모두 `{}` 자리를 가져야 한다.
    #[test]
    fn format_placeholders_match_across_languages() {
        for (name, ko, en) in [
            ("sort_maximum", KO.sort_maximum, EN.sort_maximum),
            ("sort_column_n", KO.sort_column_n, EN.sort_column_n),
        ] {
            assert_eq!(
                ko.matches("{}").count(),
                en.matches("{}").count(),
                "{name}: 자리표시자 개수가 다르다"
            );
        }
    }

    #[test]
    fn native_names_are_in_their_own_language() {
        assert_eq!(Lang::Ko.native_name(), "한국어");
        assert_eq!(Lang::En.native_name(), "English");
    }
}
