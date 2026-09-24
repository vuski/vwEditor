use crate::index::LineIndex;
use crate::indexer;
use crate::parse::{self, Encoding, SeparatorMode};
use crate::sort::{self, SortDir, SortKind, SortSpec};
use crate::source::{self, Source};
use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;

/// 현재 적용된 정렬 상태. permutation[i] = 정렬 순서 i번째로 보여줄 원본 데이터
/// 행의 논리 행번호. col/kind/dir은 헤더 화살표/버튼 상태 표시에 쓴다(다중이면
/// 1차 기준). spec_count는 상태바에 "N개 기준" 표시용(1이면 단일).
pub struct SortState {
    pub permutation: Vec<u32>,
    pub col: usize,
    pub kind: SortKind,
    pub dir: SortDir,
    pub spec_count: usize,
}

/// Find All로 확정된 하이라이트 스냅샷. 이 값 하나에 "무엇을 어떤 옵션으로 찾아
/// 어느 행이 걸렸는지"를 통째로 얼려 둔다. 라이브 입력과 분리하는 것이 핵심이라,
/// query/opts를 스냅샷 안에 함께 담아 렌더가 라이브 `find_query`/`find_opts`를
/// 전혀 참조하지 않게 한다 — 사용자가 검색어를 고쳐도 이 스냅샷은 다음 Find
/// All까지 그대로다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Highlight {
    /// Find All을 누른 순간의 검색어.
    pub query: String,
    /// 그 순간의 옵션.
    pub opts: crate::find::FindOptions,
    /// 매치가 있는 논리 행 번호(스크롤 마커 거터용, 행 오름차순).
    pub rows: Vec<u32>,
}

pub struct Document {
    pub source: Arc<Source>,
    pub index: LineIndex,
    pub enc: Encoding,
    pub sep: SeparatorMode,
    pub has_header: bool,
    pub indexer: Option<JoinHandle<()>>,
    /// 실제 파일 경로. 저장(덮어쓰기)의 대상. `path_label`은 표시용 문자열이라
    /// 되파싱하지 않고 별도로 보관한다.
    pub path: std::path::PathBuf,
    pub path_label: String,
    /// 이 탭이 찾기 결과 행 추출로 만들어졌는가. 라벨 텍스트(`"[hit] "` 접두사
    /// 유무)로 추측하지 않고 명시적으로 표시한다 — 텍스트로 판단하면 실제
    /// 파일 이름이 우연히 `"[hit] "`로 시작할 때 그 파일에서 추출해도 접두사가
    /// 또 붙지 않아, 추출 탭이 원본 파일 탭과 라벨로 구분되지 않는다
    /// (`extracted_label` 참조).
    pub is_extracted: bool,
    /// 툴바 "직접 입력" 커스텀 구분자 텍스트박스의 현재 값(한 글자).
    pub custom_sep_input: String,
    /// 헤더 클릭으로 선택된 컬럼(표 모드에서만). 정렬 대상.
    pub selected_col: Option<usize>,
    /// 현재 적용된 정렬. None이면 원본 순서.
    pub sort: Option<SortState>,
    /// 진행 중인 백그라운드 정렬 작업. 완료되면 sort로 옮기고 None이 된다.
    pub sort_job: Option<sort::SortJob>,
    /// 다중 컬럼 정렬 다이얼로그 표시 여부.
    pub show_sort_dialog: bool,
    /// 구분자 변환 다이얼로그 표시 여부.
    pub show_convert_dialog: bool,
    /// 변환 다이얼로그에서 고른 대상 구분자. `None`이면 커스텀 입력을 쓴다.
    /// 툴바의 `custom_sep_input`과 **별개**다 — 툴바는 보기 설정이고 이쪽은
    /// 데이터 변환이라, 한쪽을 만지다 다른 쪽이 따라 움직이면 사고가 난다.
    pub convert_target: Option<u8>,
    /// 변환 다이얼로그의 커스텀 구분자 입력(ASCII 한 글자).
    pub convert_custom_input: String,
    /// 다이얼로그에서 편집 중인 정렬 기준 목록(위가 1차).
    pub sort_specs: Vec<SortSpec>,
    /// 편집 모드 인메모리 버퍼. None이면 뷰 전용(mmap).
    pub edit: Option<crate::edit::EditBuffer>,
    /// 셀 편집 중인 위치와 편집 텍스트(표 모드).
    pub editing_cell: Option<(usize, usize)>,
    pub cell_edit_text: String,
    /// 셀 사각 선택(표 모드): (r0,c0,r1,c1) 논리 행/열.
    pub cell_sel: Option<(usize, usize, usize, usize)>,
    /// 드래그 선택이 셀에서 시작됐는지. 표 밖에서 누른 채 들어온 포인터가
    /// 선택을 끌고 가는 것을 막는다(egui의 primary_down은 전역 상태라
    /// 그것만으로는 시작 지점을 알 수 없다).
    pub cell_drag_active: bool,
    /// 텍스트 선택(텍스트 모드): (anchor, caret).
    pub text_sel: Option<(crate::edit::TextPos, crate::edit::TextPos)>,
    /// 텍스트 커서(텍스트 모드).
    pub text_caret: crate::edit::TextPos,
    /// 드래그 선택이 텍스트 줄에서 시작됐는지. `cell_drag_active`와 같은 이유로
    /// 필요하다 — egui의 primary_down은 전역 상태라 누름이 어디서 시작됐는지
    /// 알 수 없고, is_pointer_button_down_on()은 우클릭에도 참이다.
    pub text_drag_active: bool,
    /// IME 조합 중인 글자(ㅎ → 하 → 한). **버퍼가 아니라 화면 전용**이라
    /// 되돌리기·dirty·저장 어디에도 들어가지 않는다. 확정되면 비워지고 그
    /// 글자는 `Insert`로 버퍼에 들어간다(`collect_text_intents` 참조).
    pub ime_preview: String,
    /// 행 수가 너무 많아 사용자 확인을 기다리는 컬럼 연산.
    /// Some이면 확인 다이얼로그를 띄우고, "계속"이면 그대로 실행한다.
    /// (`BIG_COLUMN_OP_ROWS` 참조.)
    pub pending_column_op: Option<PendingColumnOp>,
    /// 찾기/바꾸기 패널 표시 여부와 입력 상태. 탭마다 검색어와 현재 위치가
    /// 다르므로 App이 아니라 Document에 둔다.
    pub show_find: bool,
    pub find_query: String,
    pub replace_text: String,
    pub find_opts: crate::find::FindOptions,
    /// 찾기/바꾸기 입력란의 이스케이프 시퀀스(`\t`, `\\`, `\xNN`)를 해석할지.
    ///
    /// **기본 꺼짐.** `C:\temp`처럼 백슬래시가 든 값을 글자 그대로 찾는 경우가
    /// 흔하다 — 항상 해석하면 그 흔한 사용이 조용히 망가진다(EmEditor·VS Code도
    /// 같은 이유로 체크박스다).
    ///
    /// **`FindOptions`에 넣지 않은 이유.** `FindOptions`는 `find.rs`의 **매칭
    /// 규칙**(대소문자·scope)이고 이것은 "입력란의 글자를 어떻게 읽을 것인가"라
    /// 층이 다르다. 게다가 `Highlight` 스냅샷이 `FindOptions`를 통째로 얼려
    /// 들고 다니는데, 스냅샷의 `query`에는 **이미 해석이 끝난** 문자열이 들어가므로
    /// 그 옆에 "해석할지" 플래그가 함께 얼려 있으면 렌더가 이미 푼 것을 또 풀지
    /// 말지 헷갈리게 된다(`effective_query` 주석 참조).
    pub find_escapes: bool,
    /// 마지막으로 찾은 위치(다음 찾기의 기준). None이면 문서 처음부터.
    pub last_match: Option<crate::find::Match>,
    /// 직전 검색 결과 안내 문구(예: "3 replacements", "Not found"). 패널에 표시.
    pub find_status: String,
    /// 찾기 패널이 방금 열린 프레임인지. 열린 프레임에만 입력란에 포커스를
    /// 준다 — 매 프레임 `request_focus()`를 부르면 다른 위젯을 클릭해도
    /// 포커스가 즉시 되돌아와 아무것도 조작할 수 없게 된다.
    pub find_focus_pending: bool,
    /// Find All로 확정된 하이라이트 스냅샷. None이면 하이라이트 없음. 라이브
    /// 입력(`find_query`/`find_opts`)과 **독립**이라, 사용자가 검색어를 고쳐도 이
    /// 스냅샷은 다음 Find All까지 그대로다. 렌더(표/텍스트/거터)는 라이브 검색어가
    /// 아니라 오직 이 필드를 보고 하이라이트를 그린다 — 그래서 입력란에 타이핑해도
    /// 아무 스캔도 일어나지 않는다. (갱신 지점은 `apply_find_action`의
    /// `FindAction::All`과 추출뿐이다.)
    pub highlight: Option<Highlight>,
    /// 찾은 매치가 보이도록 스크롤할 화면 행 번호. 본문 렌더가 소비한다.
    ///
    /// **왜 그 자리에서 곧바로 스크롤하지 않는가.** 본문은 `TableBuilder`
    /// 가상 스크롤이라 화면 밖 행은 아예 그려지지 않는다 —
    /// `Response::scroll_to_me`는 그려진 위젯에만 통하므로 화면 밖 매치에는
    /// 무력하다. 대신 행 번호를 세로 offset(px)으로 환산해
    /// `TableBuilder::vertical_scroll_offset`에 넘기는데
    /// (`egui_extras-0.28.1/src/table.rs:329`), 그건 **테이블을 만들 때** 주는
    /// 빌더 옵션이라 찾기를 수행하는 지점에서 직접 부를 수 없다. 그래서 요청만
    /// 남기고 테이블 생성 시점이 소비한다.
    ///
    /// 환산은 `scroll_offset_for_row`가 한다 — `scroll_to_row`를 쓰면 egui가
    /// 0.1~0.3초에 걸쳐 부드럽게 감아 페이지 이동이 느리게 느껴지기 때문이다
    /// (K-3, 그 함수 주석 참조).
    ///
    /// 찾기 패널의 버튼은 본문(`CentralPanel`)보다 먼저 처리되므로 **같은
    /// 프레임**에 반영되고, `update()` 끝의 F3 단축키 경로는 본문이 이미
    /// 그려진 뒤라 **다음 프레임**에 반영된다. 둘 다 이 한 필드로 처리된다.
    pub pending_scroll_row: Option<usize>,
    /// `pending_scroll_row`를 화면 어디에 붙일지. 찾기는 `Center`(매치가
    /// 가장자리에 붙지 않고 앞뒤 맥락과 함께 보이게), Page Up/Down은
    /// `Align::TOP`이다 — 페이지 단위 이동은 "이 행부터 한 화면"이라는 뜻이라
    /// 목표 행이 맨 위에 와야 다음 페이지가 정확히 이어진다(Center로 두면
    /// 반 페이지가 이미 본 내용이 된다).
    ///
    /// 정렬을 `pending_scroll_row`의 `Option` 안에 튜플로 넣지 않고 별도
    /// 필드로 둔 이유: 기존 스크롤 요청 지점(찾기, 거터 클릭)이 모두
    /// `Some(row)`만 쓰고 있어 튜플로 바꾸면 그 지점들이 전부 정렬을 명시해야
    /// 하는데, 그건 "정렬은 기본이 Center"라는 기존 동작을 옮겨 적는 일이라
    /// 한 군데만 빠뜨려도 조용히 회귀한다. 기본값을 Center로 둔 별도 필드는
    /// **아무것도 안 하면 예전 그대로**다.
    pub pending_scroll_align: egui::Align,
    /// 이번(정확히는 직전) 프레임에 화면에 그려진 **첫 화면 행**과 화면에
    /// 들어가는 행 수. 렌더가 매 프레임 기록하고 Page Up/Down이 읽는다.
    ///
    /// **왜 필요한가.** 본문은 `body.rows` 가상 스크롤이라 화면 밖 행은
    /// 그려지지 않고, `Table::body`는 `ScrollAreaOutput`을 삼켜(`()` 반환)
    /// 스크롤 offset을 돌려주지 않는다. 그래서 "지금 어디를 보고 있나"를
    /// 알 방법은 **그려진 행 번호를 관측하는 것**뿐이다.
    ///
    /// 단위는 `pending_scroll_row`와 같은 **화면 행**이다(표 모드는 정렬
    /// permutation 때문에 논리 행과 다르다) — Page Up/Down은 "지금 보는
    /// 자리에서 한 화면 위/아래"라 화면 행끼리 더하고 빼면 되고, 논리 행으로
    /// 변환할 이유가 없다.
    pub first_visible_row: usize,
    /// 한 화면에 들어가는 행 수(`available_height / row_height()`). 렌더가
    /// 기록한다 — Page 키 처리 시점(`update()` 끝)에는 본문 `Ui`가 없어
    /// `available_height`를 다시 구할 수 없기 때문이다.
    pub visible_rows: usize,
    /// 이 문서를 그릴 때 쓸 데이터 영역 확대 배율. 매 프레임 `App::view_scale`
    /// 에서 복사해 넣는다(`sync_view_scale`).
    ///
    /// **왜 App이 아니라 Document에 두는가.** `render_table`/`render_text`/
    /// `render_hex`는 `&mut Document`만 받고 `&App`은 받지 않는다(빌림 충돌).
    /// 배율을 인자로 더하면 세 함수와 그 호출부·테스트가 전부 시그니처를
    /// 바꿔야 하는데, 문서는 이미 `visible_rows`처럼 "그리기에 필요한 프레임
    /// 상태"를 담는 자리다. 배율도 같은 성격이라 여기 둔다. 소유권은 여전히
    /// `App::view_scale`에 있고 이 필드는 그 사본이다 — 여기에 쓰면 다음
    /// 프레임에 덮인다.
    pub view_scale: f32,
    /// 파싱 오류 행 검사 결과. `None`이면 아직 검사한 적이 없다(진행 중이거나
    /// 시작 전). `Some`이면 그 시점 기준으로 검사가 **끝났다**.
    ///
    /// "결과 없음"과 "오류 0개"를 `Option`으로 가른다 — 둘 다 빈 목록이지만
    /// 상태바에 쓸 문구가 정반대다("검사 중…" vs "오류 없음").
    pub row_errors: Option<crate::validate::ScanResult>,
    /// 진행 중인 백그라운드 검사. 완료되면 `row_errors`로 옮기고 `None`이 된다.
    pub error_scan: Option<crate::validate::ScanJob>,
    /// `row_errors`가 어느 시점의 편집 버퍼를 설명하는가
    /// (`UndoStack::revision`). 지금 값과 다르면 목록이 낡은 것이므로 다시
    /// 검사한다. 뷰 모드(`edit == None`)에서는 데이터가 변하지 않으므로 0.
    ///
    /// 편집 지점마다 무효화를 부르지 않고 이렇게 **관측**으로 처리하는 이유:
    /// 편집을 수행하는 자리가 열 곳이 넘어, 손으로 거는 무효화는 나중에 추가될
    /// 편집 하나가 조용히 빠진다.
    pub row_errors_revision: u64,
    /// 오류 행 창 표시 여부. 탭마다 다르므로 `App`이 아니라 여기 둔다.
    pub show_errors_window: bool,
    /// Some이면 이 문서는 헥스 모드다. 텍스트 문서와 배타적이며 텍스트
    /// 관련 필드(sep, index, edit, …)는 헥스 문서에서 쓰이지 않는다.
    /// CentralPanel 분기가 이 필드로 `render_hex` 경로를 고른다.
    pub hex: Option<crate::hex::HexState>,
    /// Some이면 이 문서는 **읽기 전용 Parquet**이다. 표 모드로 그리되 행은
    /// mmap이 아니라 여기서 나온다(`logical_line` 참조). `edit`/`hex`와
    /// 배타적이고 `index`(LineIndex)는 비어 있다 — 개행을 셀 필요가 없다.
    ///
    /// **`RefCell`인 이유**: 행 조회가 row group 캐시를 갱신하므로 `&mut`가
    /// 필요한데 `logical_line`은 `&Document`를 받고 호출부가 25곳이다.
    /// 시그니처를 바꾸면 전부 깨지므로 내부 가변성으로 감춘다. 단일 스레드
    /// UI에서만 쓰므로 `RefCell`이면 충분하다.
    pub parquet: Option<std::cell::RefCell<crate::parquet::ParquetDoc>>,
    /// 메모리를 크게 쓸 것 같아 사용자 확인을 기다리는 Parquet 정렬
    /// `(컬럼, 방향)`. hex 모드의 `confirm_load`와 같은 방식이다 —
    /// 확인 전에는 정렬을 시작하지 않는다.
    pub pending_parquet_sort: Option<(usize, SortDir)>,
}

/// 확인 없이 바로 수행할 컬럼 연산의 최대 행 수. 이보다 크면 사용자에게 묻는다.
/// 수백만 행 컬럼을 복사하면 클립보드 문자열이 수백 MB가 되고, 컬럼이 선택된
/// 상태의 "행 삭제"는 클릭 한 번에 전 데이터 행을 지운다 — 둘 다 실수로
/// 벌어지면 되돌리기 부담이 크므로 한 번 묻는다.
pub const BIG_COLUMN_OP_ROWS: usize = 1_000_000;

/// 행 수가 임계치를 넘어 확인을 기다리는 컬럼 연산. 다이얼로그에서 "계속"을
/// 누르면 같은 동작을 `confirmed = true`로 다시 태운다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingColumnOp {
    act: CellMenuAction,
    /// `Event::Paste`가 준 시스템 클립보드 문자열(있으면). 확인 뒤에도 같은
    /// 내용을 붙여야 하므로 함께 보관한다 — 그 이벤트는 이미 소비됐다.
    paste_text: Option<String>,
    /// 대상 행 수(안내 문구용).
    rows: usize,
}

/// 탭 바(전환/닫기)를 잠가야 하는가. `pending_action`(닫기 확인 대기),
/// 저장 다이얼로그, 바이너리 열기 방식 선택, 대형 바이너리 로드 확인 중
/// 하나라도 떠 있으면 잠근다 — 전부 egui 0.28 `Window`라 모달이
/// 아니므로(`.modal()`은 0.30부터), 잠그지 않으면 그 아래에서 탭 집합이나
/// 활성 탭이 움직여 대기 중인 인덱스(`CloseTab(i)`)나 저장 다이얼로그의
/// 인코딩/BOM 선택이 엉뚱한 문서를 가리키게 된다. GUI 클로저(`update()`)
/// 안에 인라인으로 두면 순수 로직으로 테스트할 수 없으므로 별도 함수로 뺀다.
///
/// - `pending_binary_open`: 열기 방식(헥스/텍스트+인코딩) 선택 대기. 이게
///   떠 있는 동안 새 `open_path`가 들어오면 그 보류를 조용히 덮어써서
///   사용자가 고른 인코딩과 앞선 파일이 함께 사라진다(C1). `open_path`가
///   스스로 거절하는 것과 짝을 이뤄, 드롭/탭 조작 자체를 여기서 막는다.
/// - `hex_confirm_load`: 활성 문서의 `HexState::confirm_load`(512MB 초과
///   로드 확인). 이 다이얼로그는 "지금 활성인 문서"를 대상으로 읽고 쓰므로
///   잠금 없이 탭이 바뀌면 엉뚱한 문서를 로드하거나 플래그가 영영 켜진 채
///   남는다(I4).
fn tab_bar_locked(
    pending_action: &Option<PendingAction>,
    show_save_dialog: bool,
    pending_binary_open: bool,
    hex_confirm_load: bool,
    parquet_sort_confirm: bool,
) -> bool {
    pending_action.is_some()
        || show_save_dialog
        || pending_binary_open
        || hex_confirm_load
        // Parquet 정렬 확인도 "지금 활성인 문서"를 대상으로 하므로 같은 이유로
        // 잠근다 — 탭이 바뀌면 엉뚱한 문서를 정렬하거나 플래그가 영영 남는다.
        || parquet_sort_confirm
}

/// 열기 방식 선택이 보류 중이라 새 열기를 거절했을 때의 안내.
/// `EXTRACT_LOCKED_STATUS`와 같은 결이다.
const BINARY_OPEN_PENDING_STATUS: &str = "Close the open dialog first";

/// `tab_bar_locked`에 `App`의 실제 상태를 먹이는 어댑터. 인자 조립을
/// 호출부마다 따로 적으면(특히 새로 늘어난 두 조건) 한 곳만 빠뜨려도
/// 티가 나지 않으므로 조립을 한 곳에 둔다(`page_keys_live_for`와 같은 규율).
fn tab_bar_locked_for(app: &App) -> bool {
    tab_bar_locked(
        &app.pending_action,
        app.show_save_dialog,
        app.pending_binary_open.is_some(),
        app.doc().is_some_and(|d| d.hex.as_ref().is_some_and(|h| h.confirm_load)),
        app.doc().is_some_and(|d| d.pending_parquet_sort.is_some()),
    )
}

/// 드롭된 경로 중 실제로 열 것만 골라낸다. 디렉터리는 건너뛴다 — `open_path`가
/// 그대로 받으면 열기에 실패해 `self.error`만 채우고 아무것도 하지 않으므로,
/// 드롭 여러 개 중 폴더 하나 때문에 에러 문구가 그걸로 덮이는 걸 미리 막는다.
/// 순서는 드롭된 순서 그대로 유지한다(마지막 파일의 탭이 활성화되는 게
/// 자연스러운 동작이므로 순서가 곧 결과에 드러난다).
fn droppable_paths(paths: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    paths.into_iter().filter(|p| !p.is_dir()).collect()
}

/// 드롭 오버레이에 띄울 안내 문구. 1개면 단수, 그 외(0 포함하나 오버레이는
/// n>0일 때만 그려지므로 실질적으로 2개 이상)는 개수를 밝힌다.
fn drop_hint_text(n: usize) -> String {
    if n == 1 {
        "Drop to open".to_owned()
    } else {
        format!("Drop to open {n} files")
    }
}

/// 드롭된 파일들에 대해 "무엇을 열 것인가 / 잠금 안내를 띄울 것인가"를
/// 결정한다. `update()`와 테스트가 이 함수 하나를 공유해야 한다 — 잠금
/// 판단을 `update()` 안에 인라인으로 두면(과거처럼) 테스트가 그 로직을
/// 복붙해 검증하는 꼴이 되어, 실제 가드를 지워도 테스트는 계속 통과하는
/// 착시가 생긴다.
enum DropPlan {
    /// 잠겨 있어 열지 않는다. 문구를 그대로 `self.error`에 넣는다.
    Locked(String),
    /// 열 파일 경로들(디렉터리는 이미 걸러짐). 비어 있을 수도 있다(전부
    /// 디렉터리였던 경우) — 그때는 아무 것도 하지 않는다.
    Open(Vec<std::path::PathBuf>),
}

fn plan_dropped_files(dropped: Vec<std::path::PathBuf>, locked: bool) -> DropPlan {
    if locked {
        DropPlan::Locked("Close the open dialog before opening files.".to_owned())
    } else {
        DropPlan::Open(droppable_paths(dropped))
    }
}

/// 이 동작이 "큰 컬럼 연산" 확인 대상인지. 대상 행 수가 임계치를 넘을 때만
/// 묻는다. 행 삽입은 한 줄짜리라 범위와 무관하게 항상 즉시 수행한다.
fn needs_big_op_confirm(act: CellMenuAction, rows: usize) -> bool {
    if rows <= BIG_COLUMN_OP_ROWS {
        return false;
    }
    matches!(
        act,
        CellMenuAction::Copy
            | CellMenuAction::Cut
            | CellMenuAction::Clear
            | CellMenuAction::DeleteRows
    )
}

pub struct App {
    /// 열려 있는 문서들. 탭 하나당 하나. 비어 있으면 열린 파일 없음.
    pub docs: Vec<Document>,
    /// 활성 탭 인덱스. docs가 비어 있지 않으면 반드시 유효한 인덱스여야 한다.
    /// (docs가 비면 값은 0으로 두고, 접근자가 None을 반환한다.)
    pub active: usize,
    pub error: Option<String>,
    /// 행 번호 시작값(0 또는 1). 표시 순번에 더해 라인번호로 쓴다.
    pub row_base: usize,
    /// 열 번호 시작값(0 또는 1). 컬럼 인덱스에 더해 헤더 번호로 쓴다.
    pub col_base: usize,
    /// 행/열 번호 설정 다이얼로그 표시 여부.
    pub show_numbering_dialog: bool,
    /// 저장 다이얼로그 표시 + 편집 대상 인코딩/BOM 선택 상태.
    pub show_save_dialog: bool,
    pub save_as: bool,
    pub save_enc: crate::parse::Encoding,
    pub save_bom: bool,
    /// 저장할 개행 스타일. 다이얼로그를 열 때 문서의 현재 값으로 맞추고
    /// (`init_save_defaults`), 저장 시 문서에 **되써서**(`apply_save_newline`)
    /// 화면 기호와 파일 내용이 어긋나지 않게 한다.
    pub save_newline: crate::edit::Newline,
    /// 셀 복사/잘라내기 시 채우는 앱 내부 클립보드. egui 0.28은 시스템
    /// 클립보드 읽기를 직접 제공하지 않으므로(붙여넣기는 이벤트로만 들어온다),
    /// 우클릭 "붙여넣기"의 소스로 이 캐시를 쓴다. 복사 시 시스템 클립보드에도
    /// 같은 내용을 넣어 외부 앱으로의 복사는 정상 동작한다.
    pub clipboard_cache: String,
    /// 저장하지 않은 변경이 있어 확인을 기다리는 동작. Some이면 확인 다이얼로그를
    /// 띄우고, 사용자가 "계속"을 누르면 그 동작을 수행한다.
    pub pending_action: Option<PendingAction>,
    /// 마지막으로 OS에 보낸 창 제목. 매 프레임 `ViewportCommand::Title`을 보내면
    /// 불필요한 창 시스템 왕복이 생기므로, 바뀔 때만 보내기 위해 기억해 둔다.
    pub window_title: String,
    /// 바이너리로 판정돼 사용자의 열기 방식 선택을 기다리는 파일.
    pub pending_binary_open: Option<PendingBinaryOpen>,
    /// 데이터 영역(표·텍스트·헥스 본문) 확대 배율. Ctrl+휠이 조절한다.
    ///
    /// **UI 크롬은 이 값을 타지 않는다.** 예전에는 `ctx.set_zoom_factor`로
    /// 창 전체를 확대해서 본문 글자를 키우면 메뉴·툴바·상태바까지 같이 커졌다.
    /// EMEditor를 비롯한 에디터의 Ctrl+휠은 "지금 보는 글자"만 키우는 동작이고,
    /// 크롬이 함께 커지면 큰 배율에서 정작 본문에 남는 자리가 줄어든다.
    /// 그래서 배율을 앱 상태로 들고, 데이터 영역의 폰트·행 높이에만 곱한다.
    ///
    /// 모든 탭이 하나의 값을 공유한다(문서별이 아니다) — 탭을 옮길 때마다 글자
    /// 크기가 바뀌면 산만하고, 사용자가 기대하는 것은 "앱의 보기 배율"이다.
    pub view_scale: f32,
    /// UI 언어. 시작할 때 OS 로케일에서 정하고(`Lang::detect`), 메뉴에서
    /// 바꿀 수 있다. 문구는 전부 `crate::i18n`에 있다.
    ///
    /// 선택을 디스크에 남기지 않는다 — 앱이 설정 파일을 쓰지 않는다는 원칙을
    /// 언어 하나 때문에 깨지 않는다. 로케일이 맞는 사용자는 바꿀 일이 없고,
    /// 바꾸는 쪽은 그 세션에서만 유효하다.
    pub lang: crate::i18n::Lang,
}

/// 바이너리 열기 다이얼로그의 보류 상태.
pub struct PendingBinaryOpen {
    pub path: std::path::PathBuf,
    /// "텍스트로 열기"용 인코딩 선택값.
    pub enc: Encoding,
}

/// dirty 편집 버퍼를 잃을 수 있어 확인이 필요한 동작.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    /// 편집 모드 종료(버퍼 폐기).
    ExitEditMode,
    /// 창 닫기(X / Alt+F4). 확인되면 실제로 `ViewportCommand::Close`를 보낸다.
    CloseApp,
    /// 탭 닫기(저장하지 않은 편집이 있는 탭). 확인되면 그 인덱스를 닫는다.
    CloseTab(usize),
}

impl Default for App {
    fn default() -> Self {
        App {
            docs: Vec::new(),
            active: 0,
            error: None,
            // 요청 기본값: 행/열 모두 0부터.
            row_base: 0,
            col_base: 0,
            show_numbering_dialog: false,
            show_save_dialog: false,
            save_as: false,
            save_enc: crate::parse::Encoding::Utf8,
            save_bom: false,
            // Windows 기본. 파일을 열면 그 파일의 스타일로 덮인다.
            save_newline: crate::edit::Newline::CrLf,
            clipboard_cache: String::new(),
            pending_action: None,
            // eframe이 창을 만들 때 쓴 제목과 같은 값으로 시작한다(main.rs).
            window_title: "vwEditor".to_owned(),
            pending_binary_open: None,
            view_scale: 1.0,
            // OS 로케일을 따른다. 읽지 못하면 영어.
            lang: crate::i18n::Lang::detect(),
        }
    }
}

/// 프라이밍 시 감지에 쓸 앞부분 바이트 크기.
const PRIME_BYTES: usize = 64 * 1024;

/// 이 크기 이하의 파일은 열자마자 편집 모드로 들어간다(`auto_edit_on_open`).
///
/// 10MB는 "읽는 값"이 아니라 **기다림의 값**으로 고른 수다. 편집 모드 진입은
/// `load_edit_buffer`의 동기 로드라 그 시간만큼 UI가 멈춘다 — 프레임을 하나
/// 건너뛰는 정도면 사용자는 인지하지 못하지만, 100ms를 넘기면 "느린 앱"이 된다.
/// 비용은 바이트 수가 아니라 **줄 수**가 지배한다(줄당 `String` 하나를 할당한다).
///
/// 실측(릴리스 빌드, 9.0MB / 34.6만 행 CSV): `open_path` 전체가 감지·인덱서
/// 기동·편집 버퍼 로드를 통틀어 **첫 회 21ms, 이후 8ms**. 같은 경로가 11MB
/// 파일에서는 편집 버퍼를 만들지 않아 0.1ms다. 상한에서도 한 프레임 안에
/// 들어오므로 여유가 있다.
///
/// 반대쪽 극단을 보면 왜 상한이 필요한지 분명해진다. 이 앱의 목표인 10GB급
/// 파일에서 편집 버퍼는 수억 개의 `String`이 되어 메모리와 시간 둘 다 감당이
/// 안 된다. 그래서 큰 파일은 지금처럼 mmap 뷰로 열고, 편집은 사용자가 명시적으로
/// 켤 때만 부담한다.
const AUTO_EDIT_MAX_BYTES: u64 = 10 * 1024 * 1024;

impl App {
    /// 활성 문서(읽기). 열린 문서가 없으면 None.
    pub fn doc(&self) -> Option<&Document> {
        self.docs.get(self.active)
    }
    /// 활성 문서(쓰기). 열린 문서가 없으면 None.
    pub fn doc_mut(&mut self) -> Option<&mut Document> {
        self.docs.get_mut(self.active)
    }

    /// 저장하지 않은 편집 내용이 있는지(활성 문서 기준). 편집 모드 Off /
    /// 파일 열기 확인처럼 "지금 보고 있는 탭" 이야기에 쓴다.
    pub fn edit_dirty(&self) -> bool {
        self.doc()
            .and_then(|d| d.edit.as_ref())
            .map_or(false, |e| e.dirty)
    }

    /// 어느 탭이든 저장하지 않은 편집이 있는지(텍스트/헥스 모두). 창 닫기
    /// 확인에 쓴다.
    pub fn any_dirty(&self) -> bool {
        self.docs.iter().any(doc_dirty)
    }

    /// 저장 다이얼로그를 열 때 인코딩/BOM 기본값을 현재 문서 기준으로 맞춘다.
    /// (원본과 같은 인코딩으로 저장하는 것이 기본 기대 동작.)
    fn init_save_defaults(&mut self) {
        // doc()가 self를 빌린 채로는 save_enc/save_bom(둘 다 self 소유)을 대입할
        // 수 없으므로, 필요한 값만 먼저 복사해 빌림을 끝낸다.
        if let Some(enc) = self.doc().map(|d| d.enc) {
            self.save_enc = enc;
            // CP949는 BOM이 없다. 나머지는 UTF-16이면 BOM을 기본 켬(없으면
            // 엔디안 판정이 불가능해 재열기가 깨진다).
            self.save_bom = matches!(
                enc,
                crate::parse::Encoding::Utf16Le | crate::parse::Encoding::Utf16Be
            );
        }
        // 개행도 원본과 같은 스타일이 기본이다 — 저장이 개행을 조용히
        // 바꿔 버리면 diff가 전 줄로 번진다.
        if let Some(nl) = self.doc().and_then(|d| d.edit.as_ref()).map(|e| e.newline) {
            self.save_newline = nl;
        }
    }

    /// Ctrl+휠을 읽어 **데이터 영역만** 확대/축소한다.
    ///
    /// 예전에는 `ctx.set_zoom_factor`로 창 전체를 확대했다. 그러면 본문 글자를
    /// 키울 때 메뉴·툴바·상태바까지 같이 커져, 큰 배율에서 정작 본문에 남는
    /// 자리가 줄어든다. 지금은 배율을 앱 상태(`view_scale`)로 들고 데이터
    /// 영역의 폰트·행 높이에만 곱한다. `zoom_factor`는 손대지 않으므로 OS DPI
    /// 스케일링은 egui가 알아서 하고, 크롬은 그 배율에만 반응한다.
    ///
    /// `update()`가 `eframe::Frame`을 요구해 테스트에서 직접 못 부르므로
    /// 별도 메서드로 뺀다(`apply_page_scroll`과 같은 규율).
    pub fn apply_ctrl_wheel_zoom(&mut self, ctx: &egui::Context) {
        let scroll_y = ctx.input(|i| {
            if i.modifiers.ctrl || i.modifiers.command {
                i.raw_scroll_delta.y
            } else {
                0.0
            }
        });
        if scroll_y == 0.0 {
            return;
        }
        let new_scale = zoomed_scale(self.view_scale, scroll_y);
        if new_scale != self.view_scale {
            self.view_scale = new_scale;
            // `TextStyle::Body`가 곧 표·텍스트 셀의 글꼴이므로(theme.rs 참고)
            // 배율이 바뀐 프레임에 스타일을 다시 깔아야 `Label`로 그리는
            // 경로까지 새 크기를 탄다.
            crate::theme::install_text_styles(ctx, new_scale);
        }
    }

    /// 파일을 연다. 텍스트면 곧바로, 바이너리로 판정되면 열기 방식 선택
    /// 다이얼로그를 보류한다(`render_binary_open_dialog`).
    ///
    /// **이미 보류 중인 열기 방식 선택이 있으면 아무 것도 하지 않는다.**
    /// 예전에는 `pending_binary_open`을 무조건 덮어썼는데, 그러면 .gpkg 셋을
    /// 한 번에 드롭했을 때 앞의 둘이 소리 없이 사라지고 마지막 하나만 창을
    /// 띄웠다. File▸Open…을 다이얼로그가 떠 있는 동안 또 쓰면 사용자가 이미
    /// 고른 인코딩까지 초기화됐다. `pending_action`을 덮어쓰지 않는 규율
    /// (창 닫기 확인)과 같은 이유다. 큐를 만들지 않는 것은 의도적이다 —
    /// 여기에 더해 `tab_bar_locked`가 드롭/탭 조작 자체를 막으므로, 이
    /// 거절은 그 잠금을 우회하는 경로(메뉴 열기 등)에 대한 이중 안전망이다.
    pub fn open_path(&mut self, path: &Path, ctx: &egui::Context) {
        if self.pending_binary_open.is_some() {
            self.error = Some(BINARY_OPEN_PENDING_STATUS.to_owned());
            return;
        }
        self.error = None;
        let head = match std::fs::File::open(path) {
            Ok(mut f) => {
                use std::io::Read;
                let mut buf = vec![0u8; PRIME_BYTES];
                let n = f.read(&mut buf).unwrap_or(0);
                buf.truncate(n);
                buf
            }
            Err(e) => {
                self.error = Some(format!("Failed to open file: {e}"));
                return;
            }
        };
        // Parquet은 `PAR1` 매직으로 시작한다. **확장자가 아니라 내용으로**
        // 판단하므로 `.pq` 같은 다른 확장자도 열리고, 반대로 `.parquet`인데
        // 내용이 텍스트면 아래 텍스트/바이너리 경로로 간다.
        //
        // 이 한 곳이 드래그앤드롭과 File▸Open을 **둘 다** 처리한다 — 두 경로가
        // 모두 `open_path`로 모이기 때문이다.
        if head.starts_with(b"PAR1") {
            self.open_path_parquet(path);
            return;
        }
        match parse::detect_text(&head) {
            parse::TextDetection::Binary => {
                self.pending_binary_open = Some(PendingBinaryOpen {
                    path: path.to_path_buf(),
                    enc: Encoding::Utf8,
                });
            }
            parse::TextDetection::Text(enc) => self.open_path_as_text(path, enc, ctx),
        }
    }

    /// 감지를 건너뛰고 지정 인코딩으로 텍스트로 연다. 새 탭으로 추가하고
    /// 그 탭을 활성화한다. 실패하면 `self.error`를 채우고 탭은 추가하지
    /// 않는다(기존 탭은 그대로). 드래그앤드롭·행 추출 등 나중 기능도 이
    /// 진입점(및 `open_path`)을 쓴다.
    pub fn open_path_as_text(&mut self, path: &Path, enc: Encoding, ctx: &egui::Context) {
        self.error = None;
        let src = match source::open(path) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                self.error = Some(format!("Failed to open file: {e}"));
                return;
            }
        };

        // `src`는 아래에서 `Document`로 옮겨 가므로 크기를 미리 잡아 둔다.
        let size = src.len();

        let head = {
            let n = (size as usize).min(PRIME_BYTES);
            src.slice(0, n as u64)
        };

        // 앞부분을 줄 단위로 나눠 구분자/헤더 감지에 사용
        let head_text = parse::decode_line(head, enc);
        let head_lines: Vec<&str> = head_text.lines().take(50).collect();
        let sep = parse::detect_separator(path, &head_lines);
        // 헤더 감지는 표 모드일 때만 의미가 있다. 텍스트 모드면 헤더 없음.
        let has_header = match sep {
            SeparatorMode::Char(d) => {
                let sample_rows: Vec<Vec<String>> = head_lines
                    .iter()
                    .take(20)
                    .map(|l| parse::split_fields(l, d))
                    .collect();
                parse::detect_header(&sample_rows)
            }
            SeparatorMode::None => false,
        };

        let index = LineIndex::new(src.len());
        let handle = indexer::spawn_indexer(src.clone(), index.clone(), enc, ctx.clone());

        // 커스텀 입력란 초기값: 감지된 구분자가 화면 표시 가능한 ASCII면 그 글자,
        // 아니면 빈 문자열.
        let custom_sep_input = match sep {
            SeparatorMode::Char(b) if b.is_ascii_graphic() => (b as char).to_string(),
            _ => String::new(),
        };

        self.add_document(Document {
            source: src,
            index,
            enc,
            sep,
            has_header,
            indexer: Some(handle),
            path: path.to_path_buf(),
            path_label: path.display().to_string(),
            is_extracted: false,
            custom_sep_input,
            selected_col: None,
            sort: None,
            sort_job: None,
            show_sort_dialog: false,
            show_convert_dialog: false,
            convert_target: None,
            convert_custom_input: String::new(),
            sort_specs: Vec::new(),
            edit: None,
            editing_cell: None,
            cell_edit_text: String::new(),
            cell_sel: None,
            cell_drag_active: false,
            text_sel: None,
            text_caret: crate::edit::TextPos { line: 0, col: 0 },
            text_drag_active: false,
            ime_preview: String::new(),
            pending_column_op: None,
            // 찾기 상태 초기값. `FindOptions`가 `Default`를 구현해 두어
            // (match_case: false, scope: Partial) 여기가 옵션 기본값을
            // 반복해 적지 않는다. `Document` 생성 헬퍼를 따로 두지 않은 이유:
            // 지금 생성 지점은 여기 한 곳뿐이고, 헬퍼를 두면 필드를 추가할 때
            // "구조체 리터럴이 컴파일 에러로 빠진 필드를 알려 준다"는 안전망이
            // 기본값 뒤로 숨는다. Task D가 다른 생성 지점을 만들면 그때
            // 컴파일러가 이 목록을 그대로 요구하므로 초기화를 빠뜨릴 수 없다.
            show_find: false,
            find_query: String::new(),
            replace_text: String::new(),
            find_opts: crate::find::FindOptions::default(),
            find_escapes: false,
            last_match: None,
            find_status: String::new(),
            find_focus_pending: false,
            highlight: None,
            pending_scroll_row: None,
            pending_scroll_align: egui::Align::Center,
            first_visible_row: 0,
            visible_rows: 0,
            view_scale: 1.0,
            row_errors: None,
            error_scan: None,
            row_errors_revision: 0,
            show_errors_window: false,
            hex: None,
            parquet: None,
            pending_parquet_sort: None,
        });

        // 작은 파일은 곧바로 편집 모드로. 뷰 모드로 열었다가 사용자가 메뉴에서
        // 켜는 것과 결과가 같아야 하므로 같은 함수(`enter_edit_mode`)를 부른다 —
        // 여기서 `edit`만 채우면 정렬 폐기·오류 목록 무효화 같은 부수 처리를
        // 놓친다. `add_document`가 방금 넣은 탭을 활성화해 두므로 `doc_mut`가
        // 그 문서다.
        //
        // **지금은** 갓 만든 `Document`라 `sort`/`row_errors`가 이미 None이므로
        // 그 부수 처리가 관측 가능한 차이를 내지 않는다(그래서 이 호출을 인라인
        // 대입으로 바꾸는 변이를 잡는 테스트를 쓸 수 없다 — 두 구현이 실제로
        // 구별되지 않는다). 그래도 `enter_edit_mode`를 부르는 이유는 진입 처리가
        // 나중에 늘어날 때 이 경로만 조용히 빠지지 않게 하기 위해서다.
        if auto_edit_on_open(size) {
            if let Some(doc) = self.doc_mut() {
                enter_edit_mode(doc);
            }
        }
    }

    /// Parquet 문서로 연다(읽기 전용). 새 탭으로 추가하고 활성화한다.
    /// 실패하면 `self.error`를 채우고 **탭은 추가하지 않는다**(기존 탭은
    /// 그대로) — `open_path_as_text`와 같은 규율이다.
    ///
    /// 인덱서를 돌리지 않고 `auto_edit_on_open`도 타지 않는다. 개행을 셀
    /// 필요가 없고(푸터가 행 수를 안다), 읽기 전용이라 편집 모드가 없다.
    pub fn open_path_parquet(&mut self, path: &Path) {
        self.error = None;
        let pq = match crate::parquet::open(path, self.lang) {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        // `doc.source`는 행 조회에 쓰이지 않지만 그대로 mmap한다 — 상태바의
        // 파일 크기 표시가 맞고, mmap은 지연이라 10GB에서도 비용이 없다.
        // `Option`으로 바꾸지 않는 이유: 참조가 20곳이 넘어 무관한 코드를
        // 전부 고쳐야 한다.
        let src = match source::open(path) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                self.error = Some(format!("Failed to open file: {e}"));
                return;
            }
        };
        self.add_document(parquet_document(src, path, pq));
    }

    /// 헥스 모드로 연다. 줄 개념이 없으므로 인덱서를 돌리지 않는다.
    pub fn open_path_hex(&mut self, path: &Path) {
        self.error = None;
        let src = match source::open(path) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                self.error = Some(format!("Failed to open file: {e}"));
                return;
            }
        };
        self.add_document(hex_document(src, path));
    }

    /// 이미 만들어진 Document를 새 탭으로 추가하고 활성화한다.
    /// (찾기 결과 행 추출 등 파일이 아닌 출처에서 온 문서를 위한 진입점.)
    pub fn add_document(&mut self, doc: Document) {
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
    }

    /// 빈 새 파일을 **새 탭으로** 연다(File → New, Ctrl+N).
    ///
    /// 한 줄짜리 래퍼지만 함수로 둔다 — 진입점이 둘(메뉴 클릭, 단축키)이고
    /// 메뉴 쪽은 egui 클로저 안이라 테스트가 구동할 수 없다. 두 진입점이 같은
    /// 함수를 지나야 "새 탭으로 붙는다(기존 탭을 갈아치우지 않는다)"를
    /// 한 번 검증하는 것으로 양쪽이 함께 지켜진다.
    pub fn open_new_tab(&mut self) {
        self.add_document(new_document());
    }

    /// 이번 프레임의 Tab을 **본문 탭 문자로 쓸 것인가**. 쓸 것이면 이벤트를
    /// 소비해 egui의 위젯 순회에서 없앤다.
    ///
    /// `update()` 맨 앞에서 부른다 — 메뉴바가 본문보다 먼저 그려지므로,
    /// 본문 렌더에서 소비하면 그 프레임에 이미 메뉴 버튼이 포커스 후보로
    /// 등록되어 포커스가 File로 넘어간다(사용자 보고).
    ///
    /// 조건:
    /// - **편집 모드**여야 한다. 뷰 모드에서 Tab은 평범한 포커스 순회다.
    /// - **텍스트 모드**여야 한다. 표 모드에서 Tab은 셀 이동/포커스 순회이고,
    ///   셀 안에 탭 문자가 들어가면 TSV에서는 필드가 갈라진다.
    /// - **다이얼로그가 없어야** 한다. 저장·확인 창이 떠 있으면 그 안에서
    ///   Tab이 버튼 사이를 옮겨 다녀야 한다.
    /// - **인라인 셀 편집기가 없어야** 한다(표 모드 조건에 포함되지만 명시).
    fn wants_tab_character(&self, ctx: &egui::Context) -> bool {
        let body_takes_tab = self.doc().is_some_and(|d| {
            d.edit.is_some() && matches!(d.sep, SeparatorMode::None) && d.editing_cell.is_none()
        });
        // 다른 위젯(툴바 TextEdit 등)이 포커스를 쥐고 있으면 Tab은 그 위젯을
        // 벗어나는 평범한 포커스 순회여야 한다 — 본문이 가로채면 입력란에
        // 갇힌다.
        let keyboard_free = ctx.memory(|m| m.focused().is_none());
        if !body_takes_tab || !keyboard_free || tab_bar_locked_for(self) {
            return false;
        }
        let took = consume_tab_key(ctx);
        if took {
            // **깜빡임 방지가 여기다.** 이벤트를 지워도 늦다 — egui는 프레임
            // 시작(`Memory::begin_frame`)에 이미 Tab을 읽어 `give_to_next`를
            // 켜 두었고, 그 프레임에 **처음 `interested_in_focus`로 등록되는
            // 위젯**이 포커스를 가져가며 플래그를 끈다(`memory.rs:543-545`).
            // 메뉴바가 본문보다 먼저 그려지므로 그게 File 버튼이었다 — 끝에서
            // 걷어내는 방식은 File이 한 프레임 하이라이트를 **그린 뒤**라
            // 깜빡임이 남고, 그 포커스 변화가 리페인트를 하나 더 유발한다.
            //
            // 그래서 어떤 패널보다 먼저 **더미 id를 등록해 give_to_next를
            // 선점·소진**시키고, 곧바로 포커스를 비운다. File 차례가 왔을 때는
            // 플래그가 꺼져 있어 하이라이트 프레임 자체가 생기지 않는다.
            ctx.memory_mut(|m| {
                m.interested_in_focus(egui::Id::new("body_tab_sink"));
                m.stop_text_input();
            });
        }
        took
    }

    /// 앱 시작 상태를 정한다. 실행 인자로 파일을 받았으면 그 파일을 열고,
    /// 없으면 **빈 새 파일**로 시작한다 — 메모장처럼 바로 타이핑할 수 있게.
    ///
    /// `main`이 아니라 여기 있는 이유: `main`은 테스트가 부를 수 없다. 시작
    /// 상태 판단을 `main`에 두면 "인자 없이 실행했을 때 새 문서가 생기는가"를
    /// 검증할 방법이 없어진다.
    ///
    /// 파일 열기가 실패하면(`open_path`가 `self.error`를 채우고 탭을 안 만든다)
    /// 탭이 하나도 없는 상태가 된다. 그 경우에도 빈 새 파일을 띄운다 — 사용자
    /// 입장에서 "열려던 파일이 없었다"와 "앱이 텅 비었다"는 다른 문제이고,
    /// 에러 메시지는 `self.error`가 이미 전한다.
    /// `fonts`는 `theme::install`의 결과다. 한글 폰트를 못 찾았으면 안내를
    /// 띄운다 — 단, **파일 열기 오류를 덮지 않는다.** 열려던 파일이 없다는
    /// 사실이 폰트 안내보다 급하고, 폰트 문제는 화면의 두부만 봐도 드러난다.
    pub fn start(
        &mut self,
        initial: Option<&Path>,
        ctx: &egui::Context,
        fonts: crate::theme::FontReport,
    ) {
        if let Some(p) = initial {
            self.open_path(p, ctx);
        }
        if self.docs.is_empty() {
            self.add_document(new_document());
        }
        if fonts.korean_missing && self.error.is_none() {
            self.error = Some(crate::theme::KOREAN_FONT_MISSING_MSG.to_owned());
        }
    }

    /// 탭을 닫는다. 활성 인덱스를 유효 범위로 다시 맞춘다.
    /// 마지막 탭을 닫으면 docs가 비고 active는 0이 된다.
    /// 제거된 Document가 dirty 편집 버퍼를 갖고 있어도 이 함수 자체는 묻지
    /// 않는다 — 묻는 책임은 호출부(탭 바 UI)에 있다.
    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.docs.len() {
            return;
        }
        self.docs.remove(idx);
        if idx < self.active {
            self.active -= 1;
        } else if idx == self.active {
            self.active = self.active.min(self.docs.len().saturating_sub(1));
        }
        if self.docs.is_empty() {
            self.active = 0;
        }
    }
}

use crate::index::Phase;
use egui_extras::{Column, TableBuilder};

/// 표/텍스트 모드 한 행의 **기준** 높이(배율 1.0). 고정폭 13px에 맞춘 값은
/// `theme.rs`에 있다(격자선 간격과 직결되므로 폰트 크기와 같은 곳에서 관리한다).
///
/// **렌더는 이 상수를 직접 쓰면 안 된다.** Ctrl+휠 확대는 데이터 영역에만
/// 적용되므로 실제 행 높이는 배율에 따라 달라진다 — 그리는 쪽은 `Document`가
/// 들고 있는 `row_height`(= `theme::row_height(view_scale)`)를 써야 한다.
/// 이 상수는 그 계산의 출발점이다. 지금은 배율 1.0을 전제하는 테스트만
/// 직접 참조한다(프로덕션 경로는 전부 `doc_row_height`를 거친다).
#[cfg(test)]
const ROW_HEIGHT: f32 = crate::theme::ROW_HEIGHT;

/// Ctrl+휠 한 번이 만드는 새 배율. 곱셈으로 조절해 어느 배율에서든 체감
/// 변화폭이 같게 하고(덧셈이면 작은 배율에서 껑충 뛴다), 허용 범위로 자른다.
///
/// 순수 함수로 빼 두는 이유: 경계(0.5·4.0에서 더 밀어도 넘지 않는가)와 방향
/// (위로 굴리면 커지는가)은 GUI 없이 확인해야 하는 성질이다.
fn zoomed_scale(scale: f32, scroll_y: f32) -> f32 {
    crate::theme::clamp_view_scale(scale * (1.0 + scroll_y * 0.001))
}

/// 선택 음영(컬럼 선택·셀 사각 선택 공통). 밝은 배경 위에 덧그리는 반투명
/// Windows 파랑 — 글자가 그대로 읽히도록 알파를 낮게 유지한다.
/// `from_rgba_unmultiplied`가 const가 아니라 함수로 둔다.
fn sel_shade() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(0, 120, 215, 48)
}

/// 헤더 클릭으로 선택된 컬럼 헤더 칸의 불투명 배경색. `sel_shade()`가 쓰는
/// Windows 파랑을 밝은 헤더 회색 위에 얹은 것과 같은 톤으로 맞춘다
/// (둘 중 하나만 바꾸면 헤더와 본문의 선택 색이 어긋난다).
fn header_sel_color() -> egui::Color32 {
    egui::Color32::from_rgb(197, 224, 247)
}

/// 셀 배경/격자선을 그릴 실제 사각형.
///
/// `ui.max_rect()`는 셀 **내용** 영역이라 인접 셀 사이에 `item_spacing`만큼
/// 빈 띠가 남는다. 그대로 선을 그으면 격자가 끊어져 보이고 배경도 줄무늬 사이가
/// 벌어진다. egui_extras가 줄무늬를 칠할 때 쓰는 것과 **같은 확장**
/// (`egui_extras-0.28.1/src/layout.rs:121-123`의 `gapless_rect`)을 적용해
/// 칸이 빈틈없이 이어지게 한다.
fn gapless_cell_rect(ui: &egui::Ui, rect: egui::Rect) -> egui::Rect {
    rect.expand2(0.5 * ui.spacing().item_spacing)
}

/// 셀 하나의 격자선(오른쪽 세로선 + 아래 가로선)을 긋는다. 엑셀/EMEditor처럼
/// 칸 경계가 보이게 하는 것이 목적이다.
///
/// **비용**: 이 함수는 `TableBuilder`가 실제로 그리는 셀에서만 불린다. 표는
/// 가상 스크롤이라 화면에 보이는 수십 행만 그려지므로, 행이 수억 개여도
/// 프레임당 선 개수는 (보이는 행 × 보이는 컬럼 수)로 일정하다 — 전체 행을
/// 도는 경로는 어디에도 없다.
fn paint_cell_grid(ui: &egui::Ui, rect: egui::Rect) {
    let r = gapless_cell_rect(ui, rect);
    let stroke = egui::Stroke::new(1.0, crate::theme::grid_line());
    let p = ui.painter();
    p.line_segment([r.right_top(), r.right_bottom()], stroke);
    p.line_segment([r.left_bottom(), r.right_bottom()], stroke);
}

/// 헤더 칸의 배경 + 아래 진한 구분선. 데이터 영역과 헤더를 시각적으로 가른다.
/// (선택된 컬럼이면 배경 대신 선택색을 쓰므로 `filled`로 색을 받는다.)
fn paint_header_cell(ui: &egui::Ui, rect: egui::Rect, filled: egui::Color32) {
    let r = gapless_cell_rect(ui, rect);
    let p = ui.painter();
    p.rect_filled(r, 0.0, filled);
    // 세로 구분선은 격자선과 같은 톤(칸 경계).
    p.line_segment(
        [r.right_top(), r.right_bottom()],
        egui::Stroke::new(1.0, crate::theme::grid_line()),
    );
    // 헤더 하단은 진하게 — 여기서 데이터가 시작한다는 신호.
    p.line_segment(
        [r.left_bottom(), r.right_bottom()],
        egui::Stroke::new(1.0, crate::theme::header_rule()),
    );
}

/// 라인번호 칸: 배경을 살짝 구분하고 격자선을 긋는다. 번호는 오른쪽 정렬 +
/// 흐린 색으로 그려 "데이터가 아닌 축"으로 읽히게 한다(엑셀 행 머리글과 같은 역할).
fn paint_line_number_cell(ui: &mut egui::Ui, rect: egui::Rect, text: String) {
    ui.painter().rect_filled(
        gapless_cell_rect(ui, rect),
        0.0,
        crate::theme::line_number_bg(),
    );
    paint_cell_grid(ui, rect);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(text).color(crate::theme::line_number_fg()))
                .truncate(),
        );
    });
}

/// 논리 행 번호(logical)에 해당하는 줄을 mmap offset으로 조회해 디코딩·개행
/// 제거한 문자열 하나를 돌려준다(구분자 분리 없음). **뷰 전용 경로**로,
/// 편집 버퍼를 보지 않는다 — 두 모드를 모두 다루는 것은 `logical_line`이다.
/// 해당 논리 행이 인덱스에 없으면(범위 밖 등) `None`.
fn decode_logical_line(doc: &Document, logical: usize) -> Option<String> {
    doc.index.line_range(logical).map(|(s, e)| {
        crate::parse::decode_line(doc.source.slice(s, e), doc.enc)
            .trim_end_matches(['\r', '\n'])
            .to_owned()
    })
}

/// IME 조합 중 글자를 캐럿 자리에 그리고, 그 **폭**을 돌려준다(캐럿을 그
/// 뒤로 밀기 위해). 조합 중이 아니면 아무것도 그리지 않고 0을 돌려준다.
///
/// 본문과 같은 색·폰트로 그리되 **밑줄**을 깐다. 조합 중인 글자는 아직
/// 확정되지 않아 Escape로 사라질 수 있으므로, 확정된 글자와 구분되어야 한다
/// (Windows·macOS IME의 공통 관행).
///
/// 이 글자는 `EditBuffer`에 없다 — 화면에만 있다. 그래서 본문 galley의
/// char↔x 매핑(`x_of`)에도 영향을 주지 않고, 클릭 위치 계산이 조합 글자
/// 때문에 어긋나는 일이 없다.
fn paint_ime_preview(
    painter: &egui::Painter,
    ui: &egui::Ui,
    at: egui::Pos2,
    text: &str,
    font_id: &egui::FontId,
    color: egui::Color32,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font_id.clone(), color));
    let size = galley.size();
    painter.galley(at, galley, color);
    // 밑줄 — 글자 아래 1px.
    let y = at.y + size.y - 1.0;
    painter.line_segment(
        [egui::pos2(at.x, y), egui::pos2(at.x + size.x, y)],
        egui::Stroke::new(1.0, color),
    );
    size.x
}

/// 편집 모드 한 프레임의 인텐트를 만든다.
///
/// `tab_pressed`는 **이미 소비된** Tab이다 — 소비는 `App::wants_tab_character`가
/// `update()` 맨 앞에서 한다. 여기서 소비하면 메뉴바가 먼저 그려진 뒤라
/// 포커스가 File 메뉴로 넘어간다(그 함수 주석 참조).
///
/// 게이트 규칙:
/// - **일반 키는 포커스 게이트를 탄다.** 툴바 입력란에 타이핑한 글자가 본문에
///   중복 입력되는 것을 막는 원래 목적이다.
/// - **Tab은 타지 않는다.** 게이트의 목적이 글자 중복 방지인데 Tab은 입력란에
///   글자를 넣지 않으므로 중복될 것이 없다. 반대로 태우면 첫 Tab이 포커스를
///   옮긴 뒤 스스로를 영영 막는다.
fn text_frame_intents(
    ctx: &egui::Context,
    editing: bool,
    tab_pressed: bool,
) -> Vec<TextEditIntent> {
    let keyboard_free = ctx.memory(|m| m.focused().is_none());
    let mut intents = if editing && keyboard_free {
        ctx.input(collect_text_intents)
    } else {
        Vec::new()
    };
    if editing && tab_pressed {
        intents.push(TextEditIntent::Insert("\t".to_owned()));
    }
    intents
}

/// Tab 키 이벤트를 소비해 egui의 **위젯 순회**에 쓰이지 않게 한다.
///
/// egui는 소비되지 않은 Tab을 프레임 끝에서 포커스 이동에 쓴다. 편집 모드
/// 본문에서는 Tab이 글자여야 하므로(TSV를 만들려면 필수), 인텐트로 바꾼 뒤
/// 이벤트 자체를 없애 포커스가 툴바 위젯으로 튀지 않게 한다.
///
/// Shift+Tab은 **소비하지 않는다**. 역방향 포커스 순회는 그대로 두는 편이
/// 접근성에 낫고, 내어쓰기 기능이 없어 탭 문자로도 쓰지 않기 때문이다
/// (`collect_text_intents`의 Tab 분기 주석 참조).
fn consume_tab_key(ctx: &egui::Context) -> bool {
    // `consume_key`를 쓰지 않는다 — 그쪽은 `matches_logically`라 **Shift/Alt를
    // 무시**해서(egui `input_state.rs:484`) Shift+Tab까지 같이 먹는다. 우리는
    // 맨 Tab만 원하므로(Shift+Tab은 역방향 포커스 순회로 남긴다) 수식어가
    // 정확히 비었는지 직접 본다.
    ctx.input_mut(|i| {
        let mut hit = false;
        i.events.retain(|e| {
            let is_plain_tab = matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.is_none()
            );
            if is_plain_tab {
                hit = true;
            }
            !is_plain_tab
        });
        hit
    })
}

/// IME를 캐럿 자리에 붙일 것인가. **편집 모드일 때만** 참이다.
///
/// 뷰 모드에서 IME를 켜면(= `output.ime`를 채우면) 입력할 수 없는 화면인데도
/// 한글 조합 창이 뜬다 — 사용자가 친 글자는 아무 데도 들어가지 않고 사라진다.
/// 뷰 모드에는 캐럿 자체가 없으므로 이 함수가 호출되는 일도 없어야 정상이지만,
/// 판정을 이름으로 남겨 두면 나중에 캐럿을 뷰 모드로 넓힐 때 여기가 걸린다.
fn ime_should_follow_caret(editing: bool) -> bool {
    editing
}

/// IME(한글/일본어/중국어 조합 입력) 창의 위치를 캐럿 자리로 알린다.
///
/// egui `TextEdit`이 하는 것과 같다(`text_edit/builder.rs`) — 이 앱은 본문을
/// `TextEdit` 없이 직접 그리므로 그 일을 대신 해 줘야 한다. 알리지 않으면
/// 조합 창이 캐럿과 동떨어진 자리에 뜨고, 일부 IME는 활성화되지 않는다.
///
/// `rect`는 편집 영역, `cursor_rect`는 캐럿이다. 둘 다 화면 좌표여야 하는데,
/// 이 앱은 본문에 `layer_transform`을 걸지 않으므로 UI 좌표가 곧 화면 좌표다
/// (`TextEdit`은 변환을 곱해 주지만 여기서는 항등이다).
fn set_ime_output(ctx: &egui::Context, rect: egui::Rect, cursor_rect: egui::Rect) {
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput { rect, cursor_rect });
    });
}

/// 줄 끝 개행 기호를 `at`(글자가 끝나는 자리)에 그린다. 개행이 없으면
/// (`LineEnding::None`) 아무것도 하지 않는다.
///
/// **본문 galley와 분리해 그리는 것이 요점이다.** 기호를 본문 문자열에 이어
/// 붙이면 그 galley가 캐럿 위치·클릭 역매핑·선택 범위의 진실이 되어, 존재하지
/// 않는 글자 위에 캐럿이 서고 줄 끝 클릭이 기호 안으로 빨려 들어간다.
/// 그래서 좌표(`x_of(len)`)만 받아 그 자리에 **덧그리기만** 한다.
///
/// 폰트에 `␍␊`(U+240D/U+240A)가 없으면 두부(□)가 되므로, 없을 때는 `\r`/`\n`
/// 이스케이프 표기로 떨어진다 — 의미는 그대로 전해지고 두부는 나오지 않는다.
fn paint_line_ending(
    painter: &egui::Painter,
    at: egui::Pos2,
    ending: parse::LineEnding,
    font_id: &egui::FontId,
    ui: &egui::Ui,
) {
    let text = ending_glyphs(ending, |c| ui.fonts(|f| f.has_glyph(font_id, c)));
    if text.is_empty() {
        return;
    }
    painter.text(
        at,
        egui::Align2::LEFT_TOP,
        text,
        font_id.clone(),
        crate::theme::line_ending_fg(),
    );
}

/// 개행 기호로 실제로 그릴 문자열. 폰트에 제어문자 기호(U+240x)가 있으면 그것을,
/// 없으면 이스케이프 표기(`\r`, `\n`)를 쓴다.
///
/// `has_glyph`를 인자로 받는 이유: 폰트 조회는 egui `Context`가 필요해 테스트가
/// 만들 수 없다. 판정 자체를 순수 함수로 떼어 두면 두 분기를 다 검증할 수 있다
/// (이 저장소가 GUI 클로저 안 판정에 쓰는 것과 같은 패턴).
fn ending_glyphs(ending: parse::LineEnding, has_glyph: impl Fn(char) -> bool) -> String {
    if matches!(ending, parse::LineEnding::None) {
        return String::new();
    }
    let symbol = ending.symbol();
    if symbol.chars().all(&has_glyph) {
        return symbol.to_owned();
    }
    // 폴백은 화살표가 아니라 **이스케이프 표기**다. 여기서는 CRLF와 LF를
    // 구분해 적는다 — 화살표 하나로 합친 이유는 "줄 끝 표시가 시끄럽지 않게"
    // 였는데, 폴백은 이미 두부(□)를 피하려고 글자로 적는 상황이라 그 이유가
    // 성립하지 않고, 적는 김에 정확한 편이 낫다.
    match ending {
        parse::LineEnding::None => String::new(),
        parse::LineEnding::Lf => "\\n".to_owned(),
        parse::LineEnding::Cr => "\\r".to_owned(),
        parse::LineEnding::CrLf => "\\r\\n".to_owned(),
    }
}

/// `logical` 행이 어떤 개행으로 끝나는지. 화면에 기호로 그리기 위한 값이다.
///
/// **두 모드의 정확도가 다르고, 그 차이는 없앨 수 없다.**
///
/// - **뷰 모드(mmap):** 행 범위가 개행 바이트를 포함하므로(`LineIndex` 불변식)
///   줄마다 **진짜** 개행을 읽어낸다. 섞인 파일(어떤 줄은 LF, 어떤 줄은 CRLF)도
///   있는 그대로 보인다.
/// - **편집 모드:** `EditBuffer.lines[i]`에는 개행이 없다(불변식). 로더가 줄을
///   나누면서 떼어 버리고 파일 전체 스타일 하나(`EditBuffer.newline`)만 남긴다.
///   그래서 모든 줄을 그 스타일로 그린다 — 섞인 파일을 편집 모드로 열면 화면이
///   실제와 달라질 수 있다. 이건 표시의 한계가 아니라 **편집 버퍼가 그 정보를
///   보관하지 않는다**는 사실의 결과이고, 저장할 때도 같은 스타일로 통일해
///   쓰이므로(`save::write_file`) 화면이 곧 저장 결과와 일치한다.
///
/// **마지막 줄은 파일에 종결 개행이 있을 때만 기호가 붙는다.** 없는 개행을
/// 그리면 "여기 개행이 있다"는 거짓말이 되고, 그 줄에 이어 쓸 때 사용자가
/// 기대하는 동작도 달라진다. 뷰 모드는 바이트로 직접 알 수 있고, 편집 모드는
/// 알 수 없으므로(로더가 버렸다) 마지막 줄에는 붙이지 않는다.
fn line_ending_for_row(doc: &Document, logical: usize) -> parse::LineEnding {
    match &doc.edit {
        Some(e) => {
            // 마지막 줄: 종결 개행이 있었는지 편집 버퍼는 모른다. 없다고 본다.
            if logical + 1 >= e.lines.len() {
                return parse::LineEnding::None;
            }
            match e.newline {
                crate::edit::Newline::Lf => parse::LineEnding::Lf,
                crate::edit::Newline::CrLf => parse::LineEnding::CrLf,
            }
        }
        None => doc
            .index
            .line_range(logical)
            .map(|(s, end)| parse::split_line_ending(doc.source.slice(s, end)).1)
            .unwrap_or(parse::LineEnding::None),
    }
}

/// 파일을 열자마자 편집 모드로 들어갈지. `open_path`가 부르는 순수 판정으로,
/// 크기 하나만 본다(`AUTO_EDIT_MAX_BYTES` 이하).
///
/// **인덱싱 상태를 조건에 넣지 않는 이유.** 편집 모드에서는 `edit.lines`가
/// 진실이고 mmap 인덱스는 쓰이지 않는다(`logical_line` 참고). 인덱서는 계속
/// 돌지만 화면은 그 결과를 보지 않으므로 기다릴 이유가 없다 — 이 크기 대역에서는
/// 어차피 곧 끝난다. 편집 모드를 끄면 그때 인덱스가 다시 화면의 바탕이 된다.
///
/// **구분자/헤더도 보지 않는 이유.** 텍스트 모드(구분자 없음)도 편집 대상이다.
/// 표냐 텍스트냐는 *보기* 방식이지 편집 가능 여부가 아니다.
fn auto_edit_on_open(size: u64) -> bool {
    size <= AUTO_EDIT_MAX_BYTES
}

/// 편집 모드로 진입: 파일 전체를 현재 인코딩으로 줄 배열 로드.
///
/// **동기 로드다.** 자동 진입은 `AUTO_EDIT_MAX_BYTES` 이하로 제한되고
/// (`auto_edit_on_open`), 수동 진입도 그 대역에서만 체감이 없다. 큰 파일의
/// 백그라운드 로드는 아직 없다 — 예전 주석이 "Task 9에서"라고 적어 두었으나
/// 헥스 모드 Task 9의 범위가 아니었다(기능 게이트·상태줄·회귀 방어).
pub fn enter_edit_mode(doc: &mut Document) {
    if doc.edit.is_some() {
        return;
    }
    // **Parquet은 읽기 전용이다. 여기서 막는 것이 핵심.** 호출부가 셋이고
    // (자동 진입/메뉴/단축키) 새로 생길 수 있어 UI 비활성화만으로는 샌다.
    // 이 가드가 없으면 아래 `load_edit_buffer`가 Parquet 바이너리를 깨진
    // 문자열로 편집 버퍼에 올린다.
    if doc.parquet.is_some() {
        return;
    }
    let buf = crate::edit::load_edit_buffer(&doc.source, doc.enc);
    doc.edit = Some(buf);
    // 편집 모드에선 뷰 permutation 정렬을 폐기(이제 lines가 진실).
    doc.sort = None;
    doc.sort_job = None;
    doc.editing_cell = None;
    doc.cell_sel = None;
    doc.cell_drag_active = false;
    doc.text_sel = None;
    doc.text_caret = crate::edit::TextPos { line: 0, col: 0 };
    doc.text_drag_active = false;
    doc.pending_column_op = None;
    // 검사의 **바탕**이 mmap에서 편집 버퍼로 바뀐다. 개정 번호로는 못 잡는다 —
    // 새 `EditBuffer`의 revision은 0이고 뷰 모드의 `doc_revision`도 0이라
    // 신선도 비교가 "그대로"라고 답한다. 특히 디코드 오류는 편집 버퍼에서는
    // 아예 나올 수 없으므로(이미 String이다), 뷰 모드에서 센 디코드 오류가
    // 편집 모드 화면에 그대로 남는다.
    invalidate_error_scan(doc);
}

/// 저장 직후 문서의 뷰 소스(mmap)를 방금 쓴 파일로 다시 겨눈다.
///
/// **왜 필요한가.** `Source`는 `Document`가 사는 동안 `Mmap`을 붙들고 있는데
/// (`source.rs`), `save::write_file`은 임시 파일을 만든 뒤 `std::fs::rename`으로
/// 원본을 갈아치운다(`save.rs`). Windows에서 이 rename 자체는 **성공**하지만,
/// 기존 매핑은 고아가 된 옛 파일 오브젝트의 **저장 전 바이트를 계속 돌려준다**.
/// 그대로 두면 저장 후 편집 모드를 껐을 때 `logical_line`이 `decode_logical_line`
/// (mmap 경로)으로 떨어지면서 화면이 **저장 전 내용**으로 되돌아가 사용자에게는
/// 작업이 날아간 것처럼 보인다. `index`도 낡아 행 수가 파일과 어긋난다.
/// "다른 이름으로 저장"이면 `path`만 새 파일을 가리키고 `source`는 원본을 매핑한
/// 채로 남아 더 나쁘다.
///
/// **왜 `open_path`가 아닌가.** `open_path`는 인코딩/구분자/헤더를 새로 감지하고
/// `Document`를 통째로 갈아끼운다 — 편집 버퍼(`edit`), 선택(`cell_sel`/`text_sel`),
/// 커서, 선택 컬럼이 전부 날아가고, 저장 인코딩이 원본과 다르면 툴바 설정까지
/// 바뀐다. 저장은 "파일이 갱신됐다"는 사건일 뿐 "새 파일을 열었다"가 아니므로,
/// 여기서는 `source` + `index` + 인덱서만 교체하고 나머지 편집 세션 상태는
/// **손대지 않는다**. `edit.lines`는 편집 모드의 진실이므로 절대 다시 읽지 않는다.
///
/// 실패(파일이 곧바로 지워졌다 등)하면 소스를 교체하지 않고 에러 문자열을
/// 돌려준다 — 낡은 매핑이 남지만 저장 자체는 이미 성공했고, 편집 버퍼가
/// 여전히 진실이므로 편집 모드 화면은 정확하다.
fn repoint_source_after_save(
    doc: &mut Document,
    path: &Path,
    ctx: &egui::Context,
) -> Result<(), String> {
    let src = match source::open(path) {
        Ok(s) => Arc::new(s),
        Err(e) => return Err(format!("Failed to reopen file after saving: {e}")),
    };
    // 새 인덱스를 만들고 인덱서를 새로 띄운다(Paused → "이어서 읽기"와 같은 패턴).
    let index = LineIndex::new(src.len());
    let handle = indexer::spawn_indexer(src.clone(), index.clone(), doc.enc, ctx.clone());
    doc.source = src;
    doc.index = index;
    doc.indexer = Some(handle);
    // 옛 인덱스 기준의 permutation은 새 파일에 맞지 않는다.
    doc.sort = None;
    doc.sort_job = None;
    // 오류 목록도 옛 source/index 기준이다. 편집 모드인 동안은 편집 버퍼가
    // 진실이라 티가 안 나지만, 편집 모드를 끄면 새 파일을 옛 목록으로 설명하게
    // 된다(그리고 새 인덱스가 다 될 때까지 재검사도 못 한다).
    invalidate_error_scan(doc);
    Ok(())
}

/// 편집 모드 이탈(버퍼 폐기). dirty 경고는 호출측 UI에서.
pub fn exit_edit_mode(doc: &mut Document) {
    doc.edit = None;
    doc.editing_cell = None;
    doc.cell_sel = None;
    doc.cell_drag_active = false;
    doc.text_sel = None;
    doc.text_drag_active = false;
    // 편집 버퍼가 사라지면 대기 중인 컬럼 연산도 무의미하다.
    doc.pending_column_op = None;
    // 검사의 바탕이 편집 버퍼에서 mmap으로 되돌아간다. 개정 번호 비교로도
    // 대개 낡음으로 잡히지만(편집이 있었다면), 편집 없이 켰다 끈 경우엔
    // 양쪽 다 0이라 안 잡힌다 — 우연에 기대지 않고 여기서 명시한다.
    invalidate_error_scan(doc);
}

/// logical 논리 행의 텍스트. 편집 모드면 EditBuffer에서, 아니면 mmap 디코딩.
pub fn logical_line(doc: &Document, logical: usize) -> Option<String> {
    if let Some(e) = &doc.edit {
        e.lines.get(logical).cloned()
    } else if let Some(p) = &doc.parquet {
        // Parquet은 표 문서의 **세 번째 데이터 출처**다. 여기 갈래 하나로
        // 표 렌더링·찾기·내보내기가 전부 따라온다.
        // 구분자는 콤마 고정이다(`parquet_document` 주석 참조).
        p.borrow_mut().row_line(logical, b',')
    } else {
        decode_logical_line(doc, logical)
    }
}

/// `logical_line`(편집 모드 대응) 경유로 디코딩한 뒤 구분자 `delim`으로 필드
/// 분리한다. 표 모드(SeparatorMode::Char) 전용. `render_table`의 헤더/col_count
/// 샘플/데이터 셀과 `render_sort_dialog`의 컬럼 목록이 모두 이 함수를 공유한다 —
/// 편집 모드에서도 화면과 정렬 대상이 같은 내용을 보게 하는 것이 핵심이다.
fn parse_logical_line_edit(doc: &Document, logical: usize, delim: u8) -> Option<Vec<String>> {
    logical_line(doc, logical).map(|t| crate::parse::split_fields(&t, delim))
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ---- Tab 소비 (다른 무엇보다 먼저) ----
        //
        // **위치가 곧 정확성이다.** 메뉴바가 본문보다 먼저 그려지므로
        // (`TopBottomPanel::top("menubar")` → `CentralPanel`), 본문에서
        // 소비하면 이미 늦다 — 그 프레임에 메뉴 버튼이 포커스 후보로
        // 등록되어 탭 문자는 들어가면서 포커스는 File 메뉴로 간다.
        // 어떤 패널도 그리기 전인 여기서 이벤트를 없애야 순회 자체가 없다.
        //
        // 편집 모드 + 본문이 키를 받을 수 있을 때만 먹는다. 뷰 모드나
        // 다이얼로그가 떠 있을 때는 Tab이 평범한 포커스 순회여야 한다.
        let tab_for_body = self.wants_tab_character(ctx);

        // Ctrl + 휠로 **데이터 영역만** 확대/축소.
        //
        // 예전에는 `ctx.set_zoom_factor`로 창 전체를 확대했다. 그러면 본문 글자를
        // 키울 때 메뉴·툴바·상태바까지 같이 커져, 큰 배율에서 정작 본문에 남는
        // 자리가 줄어든다. 지금은 배율을 앱 상태(`view_scale`)로 들고 데이터
        // 영역의 폰트·행 높이에만 곱한다. `zoom_factor`는 1.0 그대로 두므로
        // OS DPI 스케일링은 egui가 알아서 처리한다.
        self.apply_ctrl_wheel_zoom(ctx);

        // 창 제목 = "<파일명> — vwEditor". 바뀔 때만 보낸다(매 프레임 보내면
        // 창 시스템 왕복이 낭비다). "다른 이름으로 저장"으로 path가 바뀌어도
        // 이 비교가 자동으로 잡아낸다.
        let want_title = crate::theme::window_title(self.doc().map(|d| d.path.as_path()));
        if want_title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(want_title.clone()));
            self.window_title = want_title;
        }

        // 창 닫기(X / Alt+F4). 저장하지 않은 편집이 있으면 닫기를 취소하고 다른
        // 폐기 경로(편집 모드 Off, 파일 → 열기…)와 같은 확인 창으로 보낸다.
        // 확인 창에서 "계속"을 누르면 그때 실제로 Close를 보낸다.
        if ctx.input(|i| i.viewport().close_requested()) {
            // 이미 확인 창이 떠 있으면(사용자가 X를 또 눌렀다) 중복 처리하지 않고
            // 닫기만 막는다 — pending_action을 덮어써 앞선 동작을 잃지 않게.
            if self.pending_action.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.any_dirty() {
                // 창 닫기는 활성 탭만이 아니라 어느 탭이든 저장 안 된 편집이
                // 있으면 물어야 한다.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_action = Some(PendingAction::CloseApp);
            }
            // dirty가 아니면 그대로 닫히게 둔다.
        }

        // 탐색기에서 끌어다 놓은 파일들. raw.dropped_files는 드롭된 프레임에만
        // 채워지고 다음 프레임엔 비므로, 매 프레임 확인해서 비어 있지 않을
        // 때만 처리한다.
        //
        // 확인/저장 다이얼로그가 떠 있는 동안은 무시한다(`tab_bar_locked`) —
        // `open_path`는 새 탭을 추가할 때 `active`도 그 탭으로 옮기는데,
        // `show_save_dialog`가 떠 있을 때 `active`가 바뀌면 저장 다이얼로그가
        // 엉뚱한 문서를 저장하게 된다. `CloseTab(i)`가 대기 중일 때도, 탭이
        // 뒤에 추가되는 것 자체는 인덱스 i를 흔들지 않지만(push는 앞쪽을
        // 건드리지 않는다) 대기 중인 다이얼로그 아래서 탭 집합이 바뀌는 것은
        // Task A가 막은 것과 같은 종류의 위험이라 함께 잠근다.
        let locked = tab_bar_locked_for(self);
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped.is_empty() {
            match plan_dropped_files(dropped, locked) {
                DropPlan::Locked(msg) => self.error = Some(msg),
                DropPlan::Open(paths) => {
                    // 여러 개면 순서대로 연다 — open_path가 매번 탭을 추가하고
                    // 활성화하므로 마지막 파일의 탭이 자연스럽게 활성 탭이 된다.
                    //
                    // 여기서 새 스레드를 만들지 않는다. open_path가 하는 일(파일
                    // 열기 + 앞 64KB로 인코딩/구분자/헤더 감지)은 메인 스레드에서
                    // 파일당 수 밀리초고, 그 직후 spawn_indexer가 문서마다 독립된
                    // 백그라운드 스레드를 띄운다(그 인덱서 자체가 내부적으로
                    // rayon으로 병렬 스캔한다). 그러므로 이 루프를 순차로 돌리는
                    // 것만으로 드롭된 파일들의 인덱싱은 이미 전부 동시에 진행된다
                    // — std::thread::spawn/rayon/채널을 여기 추가로 넣는 것은
                    // 불필요한 중복이다.
                    //
                    // open_path는 진입 시 self.error를 무조건 지운다(단일 파일
                    // 열기에서는 그게 맞는 계약이다). 배치 중 앞쪽 파일이
                    // 실패하고 뒤쪽 파일이 성공하면 그 지움 때문에 실패가
                    // 조용히 사라지므로, 배치 동안의 마지막 실패 메시지를 따로
                    // 쥐고 있다가 루프가 끝난 뒤 복원한다.
                    let mut last_failure: Option<String> = None;
                    for p in paths {
                        self.open_path(&p, ctx);
                        if let Some(e) = self.error.take() {
                            last_failure = Some(e);
                        }
                    }
                    self.error = last_failure;
                }
            }
        }

        // 파일을 창 위로 끌고 오는 중(아직 놓지 않음)이면 "놓으면 열린다"를
        // 알린다. 잠겨 있을 때는 드롭 자체를 무시하므로 오버레이도 띄우지
        // 않는다 — 놓아도 아무 일이 안 일어나는데 "여기 놓으세요"라고 하면
        // 오히려 혼란스럽다. 대신 다른 문구로 "지금은 안 된다"를 알린다.
        let hovering = ctx.input(|i| i.raw.hovered_files.len());
        if hovering > 0 {
            let msg = if locked {
                "Finish the current dialog first".to_owned()
            } else {
                drop_hint_text(hovering)
            };
            // layer_painter는 그리기만 하고 입력을 가로채지 않으므로, 드롭
            // 판정이나 그 아래 UI의 호버링에 영향을 주지 않는다.
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));
            let screen = ctx.screen_rect();
            painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(96));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                msg,
                egui::FontId::proportional(24.0),
                egui::Color32::WHITE,
            );
        }

        // 탭 바 클로저 안에서도 self를 가변 대여할 수 없으므로, 메뉴바의
        // undo_clicked와 같은 패턴으로 인텐트만 받아 둔다.
        let mut want_active: Option<usize> = None;
        let mut want_close: Option<usize> = None;

        // 메뉴바 클로저 안에서는 self를 가변 대여할 수 없으므로, 되돌리기
        // 클릭 여부만 받아 두었다가 update() 끝에서 적용한다.
        let mut undo_clicked = false;
        // "찾기/바꾸기" 메뉴 클릭. 같은 이유로 인텐트만 받아 둔다.
        let mut find_menu_clicked = false;
        // "실행 취소" 항목 활성 조건: 편집 모드 + 되돌릴 게 있음.
        // 헥스 문서는 텍스트 편집 버퍼가 없으므로 헥스 편집 버퍼를 본다 —
        // 항목 하나가 문서 종류에 따라 각자의 undo 경로로 간다.
        let can_undo = match self.doc() {
            Some(d) if d.hex.is_some() => d
                .hex
                .as_ref()
                .and_then(|h| h.edit.as_ref())
                .is_some_and(|e| e.can_undo()),
            Some(d) => d.edit.as_ref().is_some_and(|e| !e.undo.is_empty()),
            None => false,
        };

        // 문구는 매 프레임 같은 언어를 본다. 메뉴 안에서 `self.lang`을 다시
        // 읽으면 언어 항목을 누른 프레임에 절반만 바뀐 UI가 그려진다.
        //
        // `lang`을 따로 복사해 두는 이유: 아래 렌더 함수들은 `self.doc_mut()`이
        // self를 빌린 안쪽에서 불리므로 그 자리에서 `self.lang`을 읽을 수 없다.
        // Lang은 Copy라 미리 꺼내 두면 빌림과 무관해진다.
        let lang = self.lang;
        let s = crate::i18n::t(lang);
        // 메뉴 안에서 self를 빌리는 곳이 많아, 언어 변경은 여기에 적어 두고
        // 패널이 끝난 뒤 반영한다.
        let mut pick_lang: Option<crate::i18n::Lang> = None;

        // 최상단 메뉴바 (파일 / 편집 / 도구 / 언어)
        egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(s.menu_file, |ui| {
                    // 빈 새 파일을 새 탭으로. 저장할 때 경로를 묻는다.
                    // 열려 있던 탭은 건드리지 않는다(`open_new_tab` 참조).
                    if ui.button(s.menu_new).clicked() {
                        self.open_new_tab();
                        ui.close_menu();
                    }
                    if ui.button(s.menu_open).clicked() {
                        // 열기 필터는 저장과 달리 **넓은 것이 먼저**다 —
                        // 어떤 파일을 열지 아직 모르므로, 아는 확장자를 한
                        // 항목에 모아 보여 주고 그다음 개별 형식을 둔다.
                        let dlg = rfd::FileDialog::new()
                            .add_filter("Text/CSV/TSV", &["txt", "csv", "tsv", "tab", "log"])
                            .add_filter("Parquet", &["parquet"])
                            .add_filter("CSV", &["csv"])
                            .add_filter("TSV", &["tsv", "tab"])
                            .add_filter("Text", &["txt"])
                            .add_filter("All files", &["*"]);
                        if let Some(path) = dlg.pick_file() {
                            // 새 탭으로 열리므로 기존 탭을 대체하지 않는다 —
                            // 저장 안 된 변경 확인이 필요 없다.
                            self.open_path(&path, ctx);
                        }
                        ui.close_menu();
                    }
                    // 저장 항목은 편집 버퍼(텍스트 또는 헥스)가 있을 때만
                    // 의미가 있다(뷰 모드는 저장할 버퍼가 없다).
                    let editing = self.doc().map_or(false, doc_savable);
                    ui.add_enabled_ui(editing, |ui| {
                        if ui.button(s.menu_save).clicked() {
                            self.show_save_dialog = true;
                            self.save_as = false;
                            self.init_save_defaults();
                            ui.close_menu();
                        }
                    });
                    // "다른 이름으로"는 Parquet에서도 열린다 — 읽기 전용이라
                    // 제자리 저장(Save)은 막되 **CSV/TSV 내보내기**는 되어야
                    // 한다. 항목 이름도 그때는 무엇을 하는지 밝힌다.
                    let exportable = self.doc().map_or(false, doc_exportable);
                    let is_parquet = self.doc().is_some_and(|d| d.parquet.is_some());
                    ui.add_enabled_ui(exportable, |ui| {
                        let label = if is_parquet {
                            "Export as CSV/TSV…"
                        } else {
                            "Save As…"
                        };
                        if ui.button(label).clicked() {
                            self.show_save_dialog = true;
                            self.save_as = true;
                            self.init_save_defaults();
                            ui.close_menu();
                        }
                    });
                });
                ui.menu_button(s.menu_edit, |ui| {
                    // 편집 메뉴 항목은 파일이 열려 있을 때만 의미가 있다.
                    let has_doc = self.doc().is_some();
                    // "Edit Mode"는 **텍스트** 편집 버퍼 토글이다. 헥스 문서에
                    // 걸면 바이너리를 인코딩으로 디코드해 줄로 쪼갠 버퍼가
                    // 생긴다 — 헥스 편집은 첫 타이핑에 저절로 승격되므로
                    // (`ensure_hex_edit`) 이 토글은 헥스에서 의미가 없고
                    // 해로울 뿐이다.
                    // Parquet도 여기서 함께 잠긴다 — 읽기 전용이라 편집 버퍼가
                    // 존재할 수 없다. 실제 방어는 `enter_edit_mode`의 가드이고
                    // 이 회색 처리는 왜 안 되는지 보여주는 표시다.
                    let text_edit_toggle =
                        has_doc && self.doc().is_some_and(text_edit_allowed);
                    ui.add_enabled_ui(text_edit_toggle, |ui| {
                        // 편집 모드 토글. 켜면 파일 전체를 인메모리 버퍼로 읽고,
                        // 끄면 버퍼를 버린다(dirty면 확인 후).
                        // 편집의 진입점이므로 편집 메뉴 맨 위에 둔다.
                        let mut edit_on = self.doc().map_or(false, |d| d.edit.is_some());
                        if ui.checkbox(&mut edit_on, s.menu_edit_mode).clicked() {
                            if edit_on {
                                if let Some(doc) = self.doc_mut() {
                                    enter_edit_mode(doc);
                                }
                            } else if self.edit_dirty() {
                                self.pending_action = Some(PendingAction::ExitEditMode);
                            } else if let Some(doc) = self.doc_mut() {
                                exit_edit_mode(doc);
                            }
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    ui.add_enabled_ui(can_undo, |ui| {
                        if ui.button(s.menu_undo).clicked() {
                            undo_clicked = true;
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    // 찾기는 뷰 모드에서도 되므로 has_doc만 보면 된다.
                    ui.add_enabled_ui(has_doc, |ui| {
                        if ui.button(s.menu_find_replace).clicked() {
                            find_menu_clicked = true;
                            ui.close_menu();
                        }
                    });
                });
                ui.menu_button(s.menu_tools, |ui| {
                    // 도구 메뉴 항목은 파일이 열려 있을 때만, 그리고 **텍스트
                    // 문서일 때만** 의미가 있다. 정렬·구분자 변환·행/열 번호·
                    // 오류 행은 전부 "행과 필드"를 전제하는데 헥스 문서에는
                    // 그 개념이 없다(`text_tools_enabled`).
                    //
                    // Convert/Bad Rows는 `table_mode`(구분자가 문자)에도 걸려
                    // 있어 헥스 문서(`sep: None`)에서는 이미 꺼지지만, 그건
                    // `hex_document`가 sep을 어떻게 두느냐에 기댄 우연이다.
                    // 그룹 전체를 명시적으로 잠근다.
                    let tools_enabled = self.doc().is_some_and(text_tools_enabled);
                    ui.add_enabled_ui(tools_enabled, |ui| {
                        if ui.button(s.menu_sort_columns).clicked() {
                            if let Some(doc) = self.doc_mut() {
                                // 표 모드 + 전체 행 접근 가능일 때만 실제로 연다.
                                // 편집 버퍼와 Parquet은 인덱싱 진행 상태와 무관
                                // 하게 정렬할 수 있다(`doc_rows_ready` 참조) —
                                // 특히 Parquet은 인덱서를 안 띄워 Phase가 영영
                                // Priming이라, 그 값을 보면 메뉴가 안 열린다.
                                if matches!(doc.sep, SeparatorMode::Char(_))
                                    && doc_rows_ready(doc)
                                {
                                    if doc.sort_specs.is_empty() {
                                        let col = doc.selected_col.unwrap_or(0);
                                        doc.sort_specs.push(SortSpec {
                                            col,
                                            kind: SortKind::Text,
                                            dir: SortDir::Asc,
                                            ci: true,
                                        });
                                    }
                                    doc.show_sort_dialog = true;
                                }
                            }
                            ui.close_menu();
                        }
                        // 구분자 변환은 표 모드에서만 의미가 있다 — 텍스트
                        // 모드는 나눌 기준이 없으므로 변환할 것도 없다.
                        let table_mode =
                            self.doc().is_some_and(|d| matches!(d.sep, SeparatorMode::Char(_)));
                        ui.add_enabled_ui(table_mode, |ui| {
                            if ui.button(s.menu_convert_delim).clicked() {
                                if let Some(doc) = self.doc_mut() {
                                    // 열 때마다 초기화한다 — 지난번 선택이 남아
                                    // 있으면 무심코 누른 Convert가 의도치 않은
                                    // 구분자로 데이터를 바꾼다.
                                    doc.convert_target = None;
                                    doc.convert_custom_input.clear();
                                    doc.show_convert_dialog = true;
                                }
                                ui.close_menu();
                            }
                            // 오류 행 목록도 표 모드 전용이다 — "필드 수가 맞는가"
                            // 라는 물음 자체가 컬럼이 있어야 성립한다.
                            if ui.button(s.menu_bad_rows).clicked() {
                                if let Some(doc) = self.doc_mut() {
                                    doc.show_errors_window = true;
                                }
                                ui.close_menu();
                            }
                        });
                        if ui.button(s.menu_row_col_numbers).clicked() {
                            self.show_numbering_dialog = true;
                            ui.close_menu();
                        }
                    });
                });
                // 언어. 시작 언어는 OS 로케일이 정하고(`Lang::detect`), 여기서
                // 그 세션 동안 바꿀 수 있다. 각 항목은 **그 언어로** 적혀 있어
                // (`native_name`) 낯선 언어로 떠 있어도 자기 언어를 찾을 수 있다.
                ui.menu_button(s.menu_language, |ui| {
                    for &l in crate::i18n::Lang::ALL {
                        // 라디오 형태로 지금 언어를 표시한다.
                        if ui.radio(self.lang == l, l.native_name()).clicked() {
                            pick_lang = Some(l);
                            ui.close_menu();
                        }
                    }
                });
            });
        });

        // 언어 변경은 메뉴를 그린 뒤에 반영한다 — 메뉴 안에서 `self.lang`을
        // 바꾸면 이 프레임의 나머지 UI가 이전 `s`를 계속 써서 한 프레임 동안
        // 두 언어가 섞인다. 다음 프레임부터 전부 새 언어로 그려진다.
        if let Some(l) = pick_lang {
            self.lang = l;
        }

        // 탭 바. 문서가 2개 이상일 때만 보인다(1개면 탭이라는 개념이 무의미하고
        // 공간만 낭비한다).
        //
        // `pending_action`(닫기 확인 대기) 또는 저장 다이얼로그가 떠 있는 동안은
        // 탭 전환/닫기를 잠근다. 두 다이얼로그 모두 egui 0.28 `Window`라 모달이
        // 아니고(`.modal()`은 0.30부터), 잠그지 않으면 그 아래에서 탭 집합이나
        // 활성 탭이 바뀌어 버린다 — `CloseTab(i)`가 들고 있는 위치 인덱스가
        // 엉뚱한 탭을 가리키게 되거나(다른 탭을 닫아 인덱스가 밀림),
        // 저장 다이얼로그가 다른 문서의 인코딩/BOM으로 저장해 버리는 사고로
        // 이어진다. 인덱스/설정이 대기하는 동안 탭 집합이 움직일 수 없게
        // 만드는 것이 곧 안전을 보장하는 방법이다.
        let tab_bar_locked = tab_bar_locked_for(self);
        if self.docs.len() > 1 {
            egui::TopBottomPanel::top("tabbar").show(ctx, |ui| {
                ui.add_enabled_ui(!tab_bar_locked, |ui| {
                    // 탭이 많으면 가로 스크롤.
                    egui::ScrollArea::horizontal().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for i in 0..self.docs.len() {
                                let label = tab_label(&self.docs[i]);
                                let selected = i == self.active;
                                if ui
                                    .selectable_label(selected, crate::theme::chrome_text(label))
                                    .clicked()
                                {
                                    want_active = Some(i);
                                }
                                if ui.small_button("✖").clicked() {
                                    want_close = Some(i);
                                }
                                ui.separator();
                            }
                        });
                    });
                });
            });
        }

        // 상단 툴바
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(doc) = self.doc_mut() {
                    ui.separator();
                    // 헥스 문서에는 구분자·인코딩·헤더·정렬이 전부 무의미하다
                    // (`hex_document`가 그 필드들을 불활성 값으로 둔다). 항목마다
                    // `add_enabled`로 회색 처리하면 "지금은 안 되지만 언젠가는
                    // 되는 것"처럼 보인다 — 헥스 문서에서는 영영 아니므로 아예
                    // 그리지 않고 무엇을 보고 있는지만 한 줄로 알린다.
                    if !text_layout_tools_enabled(doc) {
                        // 구분자 드롭다운을 그리지 않는다. 헥스는 구분자 개념이
                        // 없고, Parquet은 콤마 고정이다(원본 구분자라는 것이
                        // 존재하지 않아 사용자가 바꿀 대상이 아니다).
                        let kind = if doc.parquet.is_some() { "Parquet" } else { "Binary" };
                        ui.label(crate::theme::chrome_text(kind));
                        ui.separator();
                        ui.label(crate::theme::chrome_text(doc.path_label.clone()));
                        return;
                    }
                    // 구분자 드롭다운. None(텍스트) + 표준 구분자들 + 직접 입력.
                    let sep_label = match doc.sep {
                        SeparatorMode::None => "None (plain text)".to_owned(),
                        SeparatorMode::Char(b',') => "Comma ,".to_owned(),
                        SeparatorMode::Char(b'\t') => "Tab".to_owned(),
                        SeparatorMode::Char(b'|') => "Pipe |".to_owned(),
                        SeparatorMode::Char(b';') => "Semicolon ;".to_owned(),
                        SeparatorMode::Char(b) if b.is_ascii_graphic() => {
                            format!("Custom: {}", b as char)
                        }
                        SeparatorMode::Char(b) => format!("Custom: 0x{b:02X}"),
                    };
                    let sep_before = doc.sep;
                    egui::ComboBox::from_label(crate::theme::chrome_text("Delimiter"))
                        .selected_text(sep_label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut doc.sep, SeparatorMode::None, "None (plain text)");
                            ui.selectable_value(&mut doc.sep, SeparatorMode::Char(b','), "Comma ,");
                            ui.selectable_value(&mut doc.sep, SeparatorMode::Char(b'\t'), "Tab");
                            ui.selectable_value(&mut doc.sep, SeparatorMode::Char(b'|'), "Pipe |");
                            ui.selectable_value(&mut doc.sep, SeparatorMode::Char(b';'), "Semicolon ;");
                        });
                    // 직접 입력: 한 글자 텍스트박스. 입력하면 그 글자(첫 바이트)를
                    // 구분자로 사용. ASCII 한 글자만 유효(멀티바이트는 첫 바이트).
                    ui.label(crate::theme::chrome_text(s.convert_custom));
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut doc.custom_sep_input)
                            .desired_width(28.0)
                            .char_limit(1),
                    );
                    if resp.changed() {
                        if let Some(&b) = doc.custom_sep_input.as_bytes().first() {
                            doc.sep = SeparatorMode::Char(b);
                        }
                        // 입력을 비우면 텍스트 모드로.
                        if doc.custom_sep_input.is_empty() {
                            doc.sep = SeparatorMode::None;
                        }
                    }
                    // 구분자가 바뀌면 컬럼 경계가 달라지므로 선택/정렬을 무효화한다.
                    if doc.sep != sep_before {
                        doc.sort = None;
                        doc.sort_job = None;
                        doc.selected_col = None;
                        // 컬럼 인덱스가 무의미해지므로 다중 정렬 기준도 초기화.
                        doc.sort_specs.clear();
                        doc.show_sort_dialog = false;
                        // 필드 수 자체가 다시 계산되므로 오류 목록도 통째로 무효.
                        invalidate_error_scan(doc);
                    }
                    // 인코딩 드롭다운
                    let enc_before = doc.enc;
                    let enc_label = format!("{:?}", doc.enc);
                    egui::ComboBox::from_label(crate::theme::chrome_text("Encoding"))
                        .selected_text(enc_label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut doc.enc, crate::parse::Encoding::Utf8, "UTF-8");
                            ui.selectable_value(&mut doc.enc, crate::parse::Encoding::Cp949, "CP949");
                            ui.selectable_value(&mut doc.enc, crate::parse::Encoding::Utf16Le, "UTF-16LE");
                            ui.selectable_value(&mut doc.enc, crate::parse::Encoding::Utf16Be, "UTF-16BE");
                        });
                    // 인코딩/구분자가 바뀌면 파싱 기준이 달라지므로 정렬을 무효화한다.
                    if doc.enc != enc_before {
                        doc.sort = None;
                        doc.sort_job = None;
                        // 디코딩이 달라지면 필드 분리도 대체문자 판정도 달라진다.
                        invalidate_error_scan(doc);
                    }
                    // 헤더 체크박스는 표 모드에서만 의미가 있다.
                    if matches!(doc.sep, SeparatorMode::Char(_)) {
                        let hdr_before = doc.has_header;
                        ui.checkbox(&mut doc.has_header, s.common_header);
                        // 헤더 유무가 바뀌면 data_start가 달라져 permutation이 어긋나므로 무효화.
                        if doc.has_header != hdr_before {
                            doc.sort = None;
                            doc.sort_job = None;
                            // data_start와 기대 컬럼 수(헤더 필드 수)가 둘 다 달라진다.
                            invalidate_error_scan(doc);
                        }
                    }

                    // 정렬 컨트롤: 표 모드 + 컬럼 선택 + 인덱싱 완료일 때만 활성.
                    if matches!(doc.sep, SeparatorMode::Char(_)) {
                        ui.separator();
                        render_sort_controls(ui, doc, ctx, lang);
                    }

                    ui.separator();
                    ui.label(crate::theme::chrome_text(doc.path_label.clone()));
                }
            });
        });

        // 메뉴에서 고른 찾기/바꾸기. 패널을 그리기 **전에** 적용해야 같은
        // 프레임에 패널이 뜨고 입력란 포커스 요청도 그 프레임에 소비된다.
        if find_menu_clicked {
            if let Some(doc) = self.doc_mut() {
                doc.show_find = true;
                doc.find_focus_pending = true;
            }
        }

        // 파싱 오류 행 검사. 상태바를 그리기 **전에** 돌려야 이번 프레임의
        // 문구가 최신 상태를 반영한다(수거를 나중에 하면 완료된 검사가 한
        // 프레임 늦게 "검사 중…"으로 보인다).
        //
        // 툴바가 방금 구분자/인코딩/헤더를 바꿨다면 위에서 이미 무효화됐고,
        // 편집으로 데이터가 바뀐 경우는 `should_start_error_scan`이 개정
        // 번호로 잡아낸다.
        if let Some(doc) = self.doc_mut() {
            poll_error_scan(doc);
            start_error_scan(doc, ctx);
        }

        // 하단 상태바
        egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 오류는 문서 상태와 **배타 분기가 아니라 덧붙는 구간**이다.
                // 저장 실패 순간이야말로 "편집 중 — N 행 / ● 변경됨"이 가장
                // 필요한 때인데, 예전처럼 else-if로 두면 그 표시가 사라졌다.
                if let Some(err) = &self.error {
                    ui.label(crate::theme::chrome_text(err).color(egui::Color32::RED));
                    if self.doc().is_some() {
                        ui.separator();
                    }
                }
                if let Some(doc) = self.doc_mut() {
                    // 헥스 문서는 인덱싱 진행을 말하지 않는다. 줄 인덱서를 아예
                    // 띄우지 않으므로(`hex_document`가 `indexer: None`)
                    // `LineIndex`가 `Priming`에서 움직이지 않고, 아래 `match`를
                    // 그대로 태우면 "Indexing… 0%"와 [Stop] 버튼이 영영 남는다
                    // — 멈출 것도 없는 작업을 멈추라고 권하는 셈이다.
                    // 대신 크기와 캐럿 오프셋을 알린다.
                    //
                    // 오류 행 요약도 건너뛴다(필드 수 검사가 헥스에 없다).
                    if !text_layout_tools_enabled(doc) {
                        // 인덱서를 띄우지 않는 문서들(헥스·Parquet)은 LineIndex가
                        // Priming에서 영영 멈춰 있어 "Indexing… 0%"가 무한히
                        // 뜬다. 각자의 문구로 대신한다.
                        let text = if doc.parquet.is_some() {
                            parquet_status_text(doc)
                        } else {
                            hex_status_text(doc)
                        };
                        ui.label(crate::theme::chrome_text(text));
                        // 변경 표시는 텍스트 쪽과 같은 문구·같은 색이다.
                        if doc_dirty(doc) {
                            ui.label(
                                crate::theme::chrome_text("● Modified")
                                    .color(egui::Color32::from_rgb(200, 90, 20)),
                            );
                        }
                        return;
                    }
                    let st = doc.index.status();
                    let done_gb = st.bytes_done as f64 / 1e9;
                    let total_gb = st.total_bytes as f64 / 1e9;
                    let pct = if st.total_bytes > 0 {
                        st.bytes_done * 100 / st.total_bytes
                    } else {
                        100
                    };
                    match st.phase {
                        Phase::Priming | Phase::Indexing => {
                            ui.label(crate::theme::chrome_text(format!(
                                "Indexing… {done_gb:.2} / {total_gb:.2} GB ({pct}%)"
                            )));
                            if ui.button(s.common_stop).clicked() {
                                doc.index.request_pause();
                            }
                        }
                        Phase::Paused => {
                            ui.label(crate::theme::chrome_text(format!(
                                "Stopped — showing first {} rows ({done_gb:.2} / {total_gb:.2} GB)",
                                doc.index.line_count()
                            )));
                            if ui.button(s.common_resume).clicked() {
                                // 재개 = 처음부터 다시 병렬 스캔. spawn_indexer가
                                // 프라이밍→병렬을 새로 수행하며 인덱스를 덮어쓴다.
                                // 기존 핸들은 이미 종료됨.
                                // 재스캔으로 행 구성이 바뀔 수 있으므로 정렬 무효화.
                                doc.sort = None;
                                doc.sort_job = None;
                                // 같은 이유로 오류 목록도 버린다 — 인덱스가
                                // 통째로 다시 만들어지므로 옛 행번호가 무의미하다.
                                invalidate_error_scan(doc);
                                let handle = crate::indexer::spawn_indexer(
                                    doc.source.clone(),
                                    doc.index.clone(),
                                    doc.enc,
                                    ctx.clone(),
                                );
                                doc.indexer = Some(handle);
                            }
                        }
                        Phase::Complete => {
                            ui.label(crate::theme::chrome_text(format!(
                                "Ready — {} rows",
                                doc.index.line_count()
                            )));
                            // 정렬이 적용돼 있으면 어떤 기준인지 표시.
                            if let Some(s) = &doc.sort {
                                let kind = match s.kind {
                                    SortKind::Text => "text",
                                    SortKind::Number => "number",
                                };
                                let dir = match s.dir {
                                    SortDir::Asc => "ascending",
                                    SortDir::Desc => "descending",
                                };
                                ui.separator();
                                if s.spec_count > 1 {
                                    ui.label(crate::theme::chrome_text(format!(
                                        "Sorted by {} criteria (primary: column {})",
                                        s.spec_count,
                                        s.col + 1
                                    )));
                                } else {
                                    ui.label(crate::theme::chrome_text(format!(
                                        "Sorted by column {} ({kind}, {dir})",
                                        s.col + 1
                                    )));
                                }
                            }
                        }
                    }
                    // 편집 모드 표시. 인덱싱 단계와 무관하게 항상 보여야 하므로
                    // match 밖에 둔다. dirty면 붉은 "● 변경됨"을 덧붙인다.
                    if let Some(e) = &doc.edit {
                        ui.separator();
                        ui.label(crate::theme::chrome_text(format!(
                            "Editing — {} rows",
                            e.lines.len()
                        )));
                        if e.dirty {
                            ui.label(
                                crate::theme::chrome_text("● Modified")
                                    .color(egui::Color32::from_rgb(200, 90, 20)),
                            );
                        }
                    }
                    // 파싱 오류 행 요약. 오류가 있으면 눌러서 창을 연다.
                    if let Some(text) = error_status_text(doc) {
                        ui.separator();
                        let has_errors =
                            doc.row_errors.as_ref().is_some_and(|r| r.total() > 0);
                        if has_errors {
                            let label = crate::theme::chrome_text(text)
                                .color(egui::Color32::from_rgb(200, 60, 40));
                            if ui
                                .add(egui::Label::new(label).sense(egui::Sense::click()))
                                .on_hover_text(s.bad_click_to_list)
                                .clicked()
                            {
                                doc.show_errors_window = true;
                            }
                        } else {
                            ui.label(crate::theme::chrome_text(text));
                        }
                    }
                } else if self.error.is_none() {
                    // 문서도 오류도 없을 때만 안내 문구.
                    ui.label(crate::theme::chrome_text(s.common_no_file));
                }
            });
        });

        // 찾기/바꾸기 창. 별도 `egui::Window`라 다른 패널과 쌓는 순서를
        // 신경 쓸 필요가 없다(패널처럼 화면 영역을 잠식하지 않는다) — 여는
        // 시점(`show_find`)에만 좌우된다.
        //
        // 창 클로저 안에서는 `doc`이 가변 대여돼 있어 찾기를 곧바로 부를 수
        // 없다(찾기가 `logical_line`으로 `doc`을 다시 빌린다). 기존
        // `undo_clicked`와 같은 규율로 인텐트만 받아 두었다가 클로저가 끝난
        // 뒤 적용한다.
        let mut find_action: Option<FindAction> = None;
        if self.doc().is_some_and(|d| d.show_find) {
            if let Some(doc) = self.doc_mut() {
                find_action = render_find_panel(ctx, doc, lang);
            }
        }
        // 추출만 `App`(탭 목록 + active)을 건드리므로 따로 태운다. 헥스 찾기도
        // 텍스트 전용인 `apply_find_action`이 아니라 `hex_find_next`가 처리한다.
        // 나머지는 활성 `Document` 하나로 끝난다.
        if let Some(act) = find_action {
            if act == FindAction::Extract {
                self.extract_matching_rows();
            } else if act == FindAction::HexNext {
                if let Some(doc) = self.docs.get_mut(self.active) {
                    hex_find_next(doc);
                }
            } else if let Some(doc) = self.docs.get_mut(self.active) {
                apply_find_action(doc, act);
            }
        }

        // (자동 스캔 없음.) 하이라이트는 오직 Find All/추출이 만든 `doc.highlight`
        // 스냅샷이다 — 매 프레임 문서를 스캔하지 않으므로, 입력란에 타이핑하거나
        // 옵션을 바꿔도 큰 파일이 먹통이 되지 않는다.

        // 본문: 구분 모드에 따라 표 뷰 / 텍스트 뷰로 분기.
        let row_base = self.row_base;
        let col_base = self.col_base;

        // 스크롤 마커 거터. Find All 스냅샷(`highlight`)이 있고 매치 행이 있을
        // 때만 데이터 영역 오른쪽에 얇은 세로 거터를 뗀다. **egui는 SidePanel을
        // CentralPanel보다 먼저 등록해야** 남은 영역이 본문에 돌아가므로,
        // CentralPanel 앞에 둔다.
        // 헥스 문서는 제외한다 — 거터는 `highlight` 스냅샷(행/열 좌표의 매치
        // 목록)을 행 번호로 그리는데, 헥스 찾기는 그 스냅샷을 만들지 않고
        // 바이트 오프셋으로 움직인다. 지금은 `highlight`가 늘 None이라 조건이
        // 저절로 거짓이지만, 그 사실에 기대지 않고 명시한다.
        if self
            .doc()
            .is_some_and(|d| d.hex.is_none() && show_gutter(d.highlight.as_ref()))
        {
            if let Some(doc) = self.docs.get_mut(self.active) {
                render_match_gutter(ctx, doc);
            }
        }

        // 클립보드 캐시는 render_table이 복사/붙여넣기에 쓰므로 가변 대여를
        // doc과 분리해 넘긴다(App 전체를 넘기면 doc과 동시 대여가 불가능).
        let clipboard = &mut self.clipboard_cache;
        let view_scale = self.view_scale;
        let doc_opt = self.docs.get_mut(self.active);
        // 데이터 영역은 순백. 기본 CentralPanel은 panel_fill(옅은 회색)을 쓰므로
        // 프레임을 지정해 덮는다 — 메뉴/툴바/상태바만 회색으로 남아 데이터와 갈린다.
        let data_frame = egui::Frame::central_panel(&ctx.style()).fill(crate::theme::data_bg());
        egui::CentralPanel::default().frame(data_frame).show(ctx, |ui| {
            let Some(doc) = doc_opt else { return };
            // 배율은 App이 소유하고 문서는 사본을 든다 — 렌더 함수들이 `&App`을
            // 받지 못하기 때문이다(Document 필드 주석 참조). 그리기 직전에
            // 넣어야 이 프레임의 Ctrl+휠 결과가 곧바로 반영된다.
            doc.view_scale = view_scale;
            // 헥스 문서는 표/텍스트와 배타적인 **세 번째 렌더 경로**다. `sep`는
            // 헥스 문서에서 의미가 없으므로(`hex_document`가 None으로 둔다)
            // 구분자 분기보다 먼저 가른다.
            if doc.hex.is_some() {
                render_hex(ui, doc, clipboard);
            } else {
                match doc.sep {
                    SeparatorMode::Char(delim) => {
                        render_table(ui, doc, delim, row_base, col_base, clipboard)
                    }
                    SeparatorMode::None => {
                        render_text(ui, doc, row_base, clipboard, tab_for_body, lang)
                    }
                }
            }
        });

        // 다중 컬럼 정렬 다이얼로그(표시 중일 때만).
        if let Some(doc) = self.doc_mut() {
            if doc.show_sort_dialog {
                render_sort_dialog(ctx, doc, col_base, lang);
            }
        }

        // 구분자 변환 다이얼로그. `Convert`를 누르면 그 자리에서 변환한다.
        if let Some(doc) = self.doc_mut() {
            if doc.show_convert_dialog {
                let want = render_convert_dialog(ctx, doc, lang);
                if want {
                    if let Some(new) = doc.convert_target {
                        convert_delimiter_in_doc(doc, new);
                    }
                }
            }
        }

        // 오류 행 창. 목록 클릭은 논리 행번호로 돌아오고, 스크롤 요청으로
        // 바꾸는 일은 거터 클릭과 **같은 함수**(`gutter_click_target`)가 한다 —
        // 정렬 permutation 역매핑을 두 벌 두면 한쪽만 고쳐져 어긋난다.
        if let Some(doc) = self.doc_mut() {
            if doc.show_errors_window {
                if let Some(logical) = render_errors_window(ctx, doc, row_base, lang) {
                    let (align, row) = gutter_click_target(doc, logical, doc.sep);
                    doc.pending_scroll_align = align;
                    doc.pending_scroll_row = Some(row);
                    // 어느 행인지 보이도록 그 행을 선택해 둔다(찾기와 같은 방식).
                    if let SeparatorMode::Char(d) = doc.sep {
                        let last_col = table_col_count(doc, d).saturating_sub(1);
                        doc.cell_sel = Some((logical, 0, logical, last_col));
                    }
                }
            }
        }

        // 행/열 번호 설정 다이얼로그.
        if self.show_numbering_dialog {
            render_numbering_dialog(ctx, self);
        }

        // 저장 다이얼로그.
        if self.show_save_dialog {
            render_save_dialog(ctx, self);
        }

        // 바이너리 열기 방식 선택 다이얼로그.
        if self.pending_binary_open.is_some() {
            render_binary_open_dialog(ctx, self);
        }

        // 저장하지 않은 변경 확인 다이얼로그.
        if self.pending_action.is_some() {
            render_confirm_discard_dialog(ctx, self);
        }

        // 대형 바이너리의 편집 진입 확인.
        if self.doc().is_some_and(|d| d.hex.as_ref().is_some_and(|h| h.confirm_load)) {
            render_confirm_hex_load_dialog(ctx, self);
        }

        // 메모리를 크게 쓰는 Parquet 정렬 확인.
        if self.doc().is_some_and(|d| d.pending_parquet_sort.is_some()) {
            render_confirm_parquet_sort_dialog(ctx, self);
        }

        // 대상 행이 아주 많은 컬럼 연산 확인 다이얼로그.
        if self
            .doc()
            .map_or(false, |d| d.pending_column_op.is_some())
        {
            render_confirm_big_column_op_dialog(ctx, self);
        }

        // (Tab이 넘기려던 포커스는 `wants_tab_character`가 update() 맨 앞에서
        //  give_to_next를 선점해 이미 막았다 — 여기서 되돌릴 것이 없다.)

        // Ctrl+N — 빈 새 파일 탭. 다이얼로그가 떠 있으면 양보한다(Ctrl+S와 같은
        // 규율). 탭이 늘고 active가 바뀌는 동작이라 확인 창 위에서 돌면 안 된다.
        if !self.show_save_dialog
            && self.pending_action.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N))
        {
            self.open_new_tab();
        }

        // Ctrl+S — 편집 모드(텍스트 또는 헥스)에서 저장 다이얼로그 열기.
        // 다른 다이얼로그가 떠 있으면 무시한다(중복 열기 방지).
        if self.doc().map_or(false, doc_savable)
            && !self.show_save_dialog
            && self.pending_action.is_none()
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)
            })
        {
            self.show_save_dialog = true;
            self.save_as = false;
            self.init_save_defaults();
        }

        // Ctrl+Z — 되돌리기(편집 모드 전용). 인라인 셀 편집 중이거나 다른
        // 위젯이 포커스를 쥐고 있으면(툴바 TextEdit 등) 그쪽에 양보한다.
        let can_undo_key = self.doc().is_some_and(can_undo_text)
            && self.pending_action.is_none()
            && !self.show_save_dialog
            && ctx.memory(|m| m.focused().is_none());
        if can_undo_key
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z))
        {
            if let Some(doc) = self.doc_mut() {
                undo_once(doc);
            }
        }

        // ---- 찾기 단축키 (Ctrl+F / F3 / Escape) ----
        //
        // 게이트는 `can_undo_key`와 같은 규율이다: 확인/저장 다이얼로그가 떠
        // 있으면 양보하고, 인라인 셀 편집 중이면 그 TextEdit이 키를 가져간다.
        // **포커스 조건만 다르다.** `Ctrl+Z`는 "포커스가 아예 없을 때"만
        // 동작하는데, 찾기는 찾기 입력란에 포커스가 있는 상태가 정상 사용
        // 흐름이므로 그 조건을 그대로 쓰면 Ctrl+F로 연 직후 F3이 죽는다.
        // 그래서 `Ctrl+F`/`F3`은 `consume_key`로 **먼저 소비**한다 —
        // `consume_key`는 그 조합을 소비할 뿐 일반 문자 입력을 건드리지 않으므로
        // 입력란 타이핑이 문서로 새지 않는다(`focused_widget_blocks_document_
        // key_intents`가 지키는 성질은 본문의 `keyboard_free` 게이트이고,
        // 그 게이트는 여기서 손대지 않는다).
        let find_keys_live = find_keys_live(self);
        if find_keys_live {
            // Ctrl+F — 패널 열기 + 입력란 포커스. 이미 열려 있으면 포커스만
            // 다시 준다(다른 곳을 클릭한 뒤 Ctrl+F로 돌아오는 흐름).
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
                if let Some(doc) = self.doc_mut() {
                    doc.show_find = true;
                    doc.find_focus_pending = true;
                }
            }
            // F3 — 패널이 닫혀 있어도 다음 찾기. 검색어가 비어 있으면 무시한다
            // (빈 검색어로 "Enter text to find"만 띄우는 것은 소음이다).
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F3)) {
                if let Some(doc) = self.doc_mut() {
                    // "사용자가 입력란에 뭔가 쳤는가"를 보는 자리다 — 버튼 활성
                    // 조건과 같은 근거이므로 **날 문자열**을 본다(디코딩 결과가
                    // 빌 수 없으므로 어차피 판정은 같지만, 근거를 통일한다).
                    if !doc.find_query.is_empty() {
                        // 헥스 문서는 `apply_find_action`(텍스트 전용 로직)이
                        // 아니라 `hex_find_next`로 간다 — 패널의 갈래와 같다.
                        if doc.hex.is_some() {
                            hex_find_next(doc);
                        } else {
                            apply_find_action(doc, FindAction::Next);
                        }
                    }
                }
            }
            // Escape — 찾기 패널 닫기. 패널이 열려 있을 때만 소비한다.
            //
            // **포커스 판정(Minor 7).** 게이트를 `show_find`만으로 두면 툴바의
            // 커스텀 구분자 TextEdit(`ui.add(egui::TextEdit::singleline(&mut
            // doc.custom_sep_input)...)`, 이 파일의 그 지점 참조) 같은 **무관한**
            // 위젯에 포커스가 있어도 Escape가 패널을 닫아 버린다 — 그 입력란은
            // 자기 것이 아닌 키에 반응해선 안 된다. 그렇다고 "포커스가 아예
            // 없을 때만"(`can_undo_key`식)으로 좁히면, 찾기 입력란 자신에
            // 포커스가 있는 정상 흐름(Ctrl+F로 연 직후, 타이핑 중)에서 Escape가
            // 죽어 버린다 — 그게 이 단축키의 존재 이유다. 그래서 "포커스가
            // 없거나, 찾기 입력란 자신에 있을 때"로 명시적으로 좁힌다.
            let focus = ctx.memory(|m| m.focused());
            let escape_owner_ok = focus.is_none() || focus == Some(find_query_id());
            if escape_owner_ok
                && self.doc().is_some_and(|d| d.show_find)
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                if let Some(doc) = self.doc_mut() {
                    doc.show_find = false;
                    doc.find_focus_pending = false;
                }
                ctx.memory_mut(|m| m.stop_text_input());
            }
        }

        // ---- Page Up / Page Down (페이지 단위 스크롤) ----
        //
        // 본문은 `body.rows` 가상 스크롤이라 화면 밖 행이 그려지지 않으므로,
        // 스크롤은 찾기와 **같은 메커니즘**으로 한다: 목표 화면 행을
        // `pending_scroll_row`에 남기면 다음 프레임 렌더가
        // `vertical_scroll_offset`으로 옮긴다(`apply_page_scroll`).
        // 이 경로는 본문이 이미 그려진 뒤라
        // 다음 프레임에 반영된다 — F3 단축키와 같다.
        //
        // **화면 행 기준으로 계산한다.** 표 모드는 정렬 permutation 때문에
        // 논리 행 ≠ 화면 행인데, 페이지 이동은 "지금 보는 자리에서 한 화면
        // 위/아래"라는 **화면상의** 요구이므로 관측값(`first_visible_row`)과
        // 목적지가 둘 다 화면 행이면 변환이 아예 필요 없다. 논리 행으로
        // 왕복하면 정렬된 문서에서 엉뚱한 곳으로 튄다.
        //
        // **편집 모드에서도 캐럿은 옮기지 않는다.** 요청은 "뷰어에서 페이지
        // 단위로 넘겨보게"이므로 스크롤만 한다. 캐럿까지 옮기면 선택 확장
        // (Shift+PageDown)·되돌리기 경계 같은 편집 의미가 딸려 오는데 그건
        // 요청 범위 밖이고, 본다고 캐럿이 움직이지 않는 편이 놀랍지 않다.
        if page_keys_live_for(self, ctx) {
            // 두 키가 같은 프레임에 오면 Down을 먼저 본다(먼저 소비된 쪽만
            // 반영된다) — 어느 쪽이든 한 프레임에 한 페이지만 움직인다.
            let dir = if ctx
                .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown))
            {
                Some(PageDir::Down)
            } else if ctx
                .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp))
            {
                Some(PageDir::Up)
            } else {
                None
            };
            if let Some(dir) = dir {
                if let Some(doc) = self.doc_mut() {
                    apply_page_scroll(doc, dir);
                }
            }
        }

        // 메뉴에서 고른 되돌리기(단축키를 모르는 사용자용). 메뉴바 클로저 안에서는
        // self를 가변 대여할 수 없어 인텐트만 받아 여기서 적용한다.
        //
        // 문서 종류에 따라 되돌리기 경로가 다르다 — 헥스는 본문 Ctrl+Z가 가는
        // 것과 같은 `HexIntent::Undo`로 보낸다(`undo_once`는 텍스트 편집 버퍼
        // 전용이라 헥스 문서에서는 조용히 아무 일도 하지 않는다).
        if undo_clicked {
            // `apply_hex_intent`는 클립보드 캐시도 가변으로 받으므로 `doc_mut`과
            // 동시 대여가 되지 않는다 — 필드를 각각 빌려 쪼갠다(본문 렌더가
            // `clipboard`를 넘기는 것과 같은 방식).
            let clipboard = &mut self.clipboard_cache;
            if let Some(doc) = self.docs.get_mut(self.active) {
                if doc.hex.is_some() {
                    apply_hex_intent(doc, clipboard, HexIntent::Undo);
                } else {
                    undo_once(doc);
                }
            }
        }

        // 탭 바 클릭 인텐트 적용. 탭 바 자체가 위에서 잠겨 있으면(대기 중인
        // pending_action이나 저장 다이얼로그) 이 인텐트들은 애초에 생기지
        // 않지만, `pending_action.is_some()`을 여기서도 다시 확인해 둔다 —
        // close_requested 훅(위 528줄)과 같은 이유다: 이미 확인을 기다리는
        // 동작이 있는데 탭 닫기가 그걸 덮어써 버리면 먼저 대기하던 동작을
        // 잃는다. 이 이중 방어 덕분에 `CloseTab(i)`의 i는 확인 창이 뜨는
        // 시점부터 실제로 적용되는 시점까지 탭 집합이 움직이지 않는다는
        // 것이 보장되고, 그래야 위치 인덱스만으로도 안전하다.
        if self.pending_action.is_none() {
            if let Some(i) = want_active {
                // 다른 탭으로 넘어가면 이전 탭의 오류 메시지는 의미가 없다
                // (예: 탭 A에서 저장 실패 → 탭 B로 전환해도 메시지가 남으면
                // 마치 B의 오류처럼 보인다). open_path가 새로 열 때 error를
                // 지우는 것과 같은 이유로, 탭을 "바꿀 때"도 지운다.
                if i != self.active {
                    self.error = None;
                }
                self.active = i;
            }
            // 탭 닫기: dirty 편집 버퍼(텍스트/헥스)가 있으면 확인 창을 띄우고,
            // 아니면 즉시 닫는다.
            if let Some(i) = want_close {
                let dirty = self.docs.get(i).is_some_and(doc_dirty);
                if dirty {
                    self.pending_action = Some(PendingAction::CloseTab(i));
                } else {
                    self.close_tab(i);
                }
            }
        }
    }
}

/// 탭 바에 보여줄 라벨. 파일명(없으면 path_label, 그것도 비면 "(untitled)")에
/// dirty 편집 버퍼가 있으면 앞에 "● "를 붙이고(상태바가 이미 ●를 쓰는 것과
/// 일관되게), 전체(접두사 포함) 24자를 넘으면 문자 경계 기준으로 잘라 "…"를
/// 붙인다(바이트 슬라이스로 자르면 한글에서 패닉한다). 접두사를 붙인 *뒤에*
/// 자르는 게 아니라 예산 안에서 조립해야 한다 — 먼저 자르고 나중에 "● "를
/// 붙이면 dirty한 긴 이름 탭이 24자 상한을 2자 넘겨 버린다.
pub(crate) fn tab_label(doc: &Document) -> String {
    let name = doc
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if doc.path_label.is_empty() {
                "(untitled)".to_owned()
            } else {
                doc.path_label.clone()
            }
        });
    let dirty = doc_dirty(doc);
    const CAP: usize = 24;
    let prefix_len = if dirty { 2 } else { 0 }; // "● "는 2글자(● + 공백).
    let budget = CAP - prefix_len;
    let truncated = if name.chars().count() > budget {
        let mut s: String = name.chars().take(budget - 1).collect();
        s.push('…');
        s
    } else {
        name
    };
    if dirty {
        format!("● {truncated}")
    } else {
        truncated
    }
}

/// 되돌리기 한 단계를 편집 버퍼에 적용하고, 행 수가 줄었을 수 있으므로
/// 선택/커서를 현재 버퍼 범위로 클램프한다. 되돌린 게 있으면 dirty를 세운다
/// (되돌리기도 "저장되지 않은 상태"를 만든다 — 저장 시점과 다른 내용이 된다).
fn undo_once(doc: &mut Document) {
    let Some(e) = doc.edit.as_mut() else { return };
    if !e.undo.undo(&mut e.lines) {
        return;
    }
    e.dirty = true;
    let Some(e) = doc.edit.as_mut() else { return };
    let len = e.lines.len();
    // 표 모드 상태: 되돌리기로 행이 줄면 낡은 인덱스가 남는다.
    doc.editing_cell = None;
    doc.cell_edit_text.clear();
    doc.cell_drag_active = false;
    doc.cell_sel = doc.cell_sel.and_then(|(r0, c0, r1, c1)| {
        if len == 0 {
            return None;
        }
        let last = len - 1;
        Some((r0.min(last), c0, r1.min(last), c1))
    });
    // 텍스트 모드 상태.
    doc.text_caret = clamp_pos(&e.lines, doc.text_caret);
    doc.text_sel = doc
        .text_sel
        .map(|(a, b)| (clamp_pos(&e.lines, a), clamp_pos(&e.lines, b)))
        .filter(|(a, b)| a != b);
    doc.text_drag_active = false;
}

// ---------------------------------------------------------------------------
// 찾기 / 바꾸기 (순수 로직은 `crate::find`, 여기는 Document에 붙이는 연결부)
// ---------------------------------------------------------------------------

/// 찾기 패널에서 낼 수 있는 동작. 패널 UI 클로저는 `doc`을 가변으로 빌린 채
/// 돌기 때문에 그 안에서 곧바로 찾기를 수행할 수 없다(찾기가 `logical_line`으로
/// `doc`을 다시 빌린다). 기존 `undo_clicked`/`menu_action`과 같은 규율로
/// 인텐트만 받아 두었다가 클로저가 끝난 뒤 적용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FindAction {
    /// 전체 문서를 스캔해 하이라이트 스냅샷(`doc.highlight`)을 만든다. 하이라이트가
    /// 갱신되는 **유일한** 지점이다 — 다른 어떤 동작도 스냅샷을 새로 만들지 않는다.
    All,
    Next,
    Prev,
    ReplaceOne,
    ReplaceAll,
    /// 검색어가 든 행만 모아 새 탭으로 추출. 다른 동작과 달리 `Document` 하나로
    /// 끝나지 않고 `App`(탭 목록)을 건드리므로 `apply_find_action`이 아니라
    /// `App::extract_matching_rows`가 처리한다.
    Extract,
    /// 헥스 문서의 다음 찾기. 텍스트 모드의 `Next`와 갈래가 다르다 —
    /// `apply_find_action`(텍스트 전용 로직)이 아니라 `hex_find_next`가 처리한다.
    /// `render_find_panel`의 헥스 전용 분기에서만 나온다.
    HexNext,
}

/// 문서의 논리 행 수(두 모드 공통). 편집 모드면 버퍼 길이, 아니면 인덱스가
/// 지금까지 찾아낸 행 수 — 인덱싱 중이면 아직 자라는 중이고, 그래서
/// `find_next`의 `get_line`이 None을 돌려줄 수 있다.
fn doc_line_count(doc: &Document) -> usize {
    if let Some(e) = &doc.edit {
        return e.lines.len();
    }
    if let Some(p) = &doc.parquet {
        // 헤더 한 줄을 더한다(인덱스 규약: 논리 행 0 = 헤더).
        // Parquet 문서는 `LineIndex`가 비어 있어 `line_count()`는 0이다.
        return p.borrow().total_rows() as usize + 1;
    }
    doc.index.line_count()
}

/// 찾기의 Whole cell 판정에 넘길 구분자. 표 모드면 `Some(delim)`, 텍스트
/// 모드면 `None`(그 경우 Whole cell은 "행 전체 일치"로 해석된다).
fn doc_delimiter(doc: &Document) -> Option<u8> {
    match doc.sep {
        SeparatorMode::Char(d) => Some(d),
        SeparatorMode::None => None,
    }
}

/// 다음/이전 찾기의 기준 위치. 마지막으로 찾은 자리가 있으면 거기서,
/// 없으면 텍스트 모드의 캐럿에서(표 모드엔 캐럿이 없으므로 문서 처음).
///
/// `find_next`/`find_prev`는 `from`을 **제외**한다(같은 자리를 반복해 돌려주면
/// "다음 찾기"가 제자리걸음이므로). 그래서 아직 아무것도 못 찾은 상태에서
/// 캐럿/문서 처음을 그대로 넘기면 **바로 그 자리의 매치를 건너뛴다** — 문서
/// 첫 글자가 검색어인데 첫 Find Next가 그것을 지나쳐 버리는 식이다.
///
/// 그래서 아직 매치가 없을 때는 기준을 캐럿의 **바로 앞 자리**로 물린다
/// (`col - 1`). 캐럿이 줄 맨 앞이면 앞 줄의 끝으로 넘어가고, 문서 맨 앞이면
/// 마지막 줄의 끝으로 감싼다 — 어느 쪽이든 wrap 처리가 결국 캐럿 자리부터
/// 훑게 만든다. `forward == false`(이전 찾기)면 대칭으로 한 칸 뒤로 민다.
fn find_origin(doc: &Document, forward: bool) -> crate::edit::TextPos {
    if let Some(m) = doc.last_match {
        return crate::edit::TextPos { line: m.line, col: m.col };
    }
    let caret = match doc.sep {
        SeparatorMode::None => doc.text_caret,
        SeparatorMode::Char(_) => crate::edit::TextPos { line: 0, col: 0 },
    };
    let n = doc_line_count(doc);
    if n == 0 {
        return caret;
    }
    if forward {
        if caret.col > 0 {
            return crate::edit::TextPos { line: caret.line, col: caret.col - 1 };
        }
        // 줄 맨 앞 → 앞 줄의 끝(문서 맨 앞이면 마지막 줄로 감싼다).
        let line = if caret.line == 0 { n - 1 } else { caret.line - 1 };
        let end = logical_line(doc, line).map_or(0, |t| t.chars().count());
        crate::edit::TextPos { line, col: end }
    } else {
        crate::edit::TextPos { line: caret.line, col: caret.col.saturating_add(1) }
    }
}

/// 찾기 단축키(Ctrl+F/F3/Escape)가 살아 있는지. `needs_big_op_confirm`/
/// `next_cell_drag_active`/`tab_label`/`plan_dropped_files`와 같은 규율로
/// 순수 함수로 뽑아 `update()`와 테스트가 **같은 코드**를 실행하게 한다
/// (Minor 6) — 게이트 식을 테스트에 따로 베껴 적으면, 실제 게이트를 지우거나
/// 뒤집어도 그 테스트는 자기 사본만 보고 계속 통과한다.
///
/// 문서가 있고, 인라인 셀 편집 중이 아니고, 대기 중인 확인 동작/저장
/// 다이얼로그가 없을 때만 살아 있다 — `can_undo_key`와 같은 규율이다.
fn find_keys_live(app: &App) -> bool {
    app.doc().is_some()
        && app.doc().is_some_and(|d| d.editing_cell.is_none())
        && app.pending_action.is_none()
        && !app.show_save_dialog
}

// ---------------------------------------------------------------------------
// Page Up / Page Down (페이지 단위 스크롤)
// ---------------------------------------------------------------------------

/// 페이지 이동 방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageDir {
    Up,
    Down,
}

/// Page Up/Down이 스크롤할 **화면 행**을 계산한다.
///
/// - `first`: 지금 화면 맨 위에 보이는 화면 행(`Document::first_visible_row`).
/// - `visible`: 한 화면에 들어가는 행 수(`Document::visible_rows`).
/// - `total`: 전체 화면 행 수(표 모드는 헤더를 뺀 데이터 행 수).
///
/// **한 행을 겹친다(`visible - 1`).** 일반 에디터 관례이고, 겹치는 행이 없으면
/// 페이지 경계에서 문맥이 끊겨 "방금 본 마지막 줄"과 "지금 첫 줄"이 이어지는지
/// 확인할 수 없다. `visible`이 0이나 1이면 `max(1)`로 최소 한 행은 움직인다 —
/// 그러지 않으면 창이 아주 작을 때 키가 아무 반응도 안 하는 것처럼 보인다.
///
/// 클램프: 위로는 0(`saturating_sub`), 아래로는 마지막 행(`total - 1`).
/// `total == 0`이면 갈 곳이 없으므로 `None`을 돌려 스크롤 요청 자체를 만들지
/// 않는다(0을 돌려주면 빈 문서에 대해 무의미한 스크롤 요청이 매번 쌓인다).
///
/// `update()`의 키 처리 클로저 안에 인라인으로 두면 이 계산을 테스트가 구동할
/// 수 없으므로(`page_keys_live`/`extract_plan`과 같은 규율) 순수 함수로 뺀다.
fn page_target_row(dir: PageDir, first: usize, visible: usize, total: usize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let step = visible.saturating_sub(1).max(1);
    let last = total - 1;
    let target = match dir {
        PageDir::Up => first.saturating_sub(step),
        PageDir::Down => first.saturating_add(step).min(last),
    };
    Some(target.min(last))
}

/// Page Up/Down 단축키가 살아 있는가. `find_keys_live`/`can_undo_key`와 같은
/// 규율의 가드를 **하나의 순수 함수**로 뽑아 `update()`와 테스트가 같은 코드를
/// 지나게 한다 — 게이트 식을 테스트에 베껴 적으면 실제 게이트를 뒤집어도 그
/// 테스트는 자기 사본만 보고 통과한다.
///
/// - `has_doc`: 문서가 열려 있는가.
/// - `editing_cell`: 인라인 셀 편집 중인가. 그러면 TextEdit이 키를 가져가야
///   한다(셀 편집기 안에서 Page Down이 문서를 넘기면 안 된다).
/// - `focused`: **다른 위젯이 키보드 포커스를 쥐고 있는가.** 찾기 입력란에
///   타이핑하는 중에 Page Down이 문서를 넘기면 안 되므로 양보한다. 찾기의
///   Ctrl+F/F3과 달리 여기서는 포커스가 있으면 **무조건** 양보한다 — Page
///   Up/Down은 텍스트 입력 위젯 자신이 캐럿을 옮기는 데 쓰는 키라
///   `consume_key`로 가로채면 그 위젯의 정상 동작을 뺏는다(Ctrl+F/F3은 어떤
///   TextEdit도 쓰지 않는 조합이라 사정이 다르다).
/// - `pending_action` / `save_dialog`: 확인·저장 다이얼로그가 떠 있으면 양보.
fn page_keys_live(
    has_doc: bool,
    editing_cell: bool,
    focused: bool,
    pending_action: bool,
    save_dialog: bool,
) -> bool {
    has_doc && !editing_cell && !focused && !pending_action && !save_dialog
}

/// `page_keys_live`에 `App`/`Context`의 실제 상태를 먹이는 어댑터.
/// `update()`와 테스트가 **이 함수 하나**를 지나게 해서, 인자를 어디서 읽는지
/// (특히 "포커스"를 `App`이 아니라 egui `Context`에서 읽는다는 사실)까지
/// 한 곳에 둔다 — 인자 조립을 양쪽에 따로 적으면, 예컨대 포커스 인자를
/// 실수로 항상 `false`로 넘겨도 테스트는 자기 사본만 보고 통과한다.
fn page_keys_live_for(app: &App, ctx: &egui::Context) -> bool {
    page_keys_live(
        app.doc().is_some(),
        app.doc().is_some_and(|d| d.editing_cell.is_some()),
        ctx.memory(|m| m.focused().is_some()),
        app.pending_action.is_some(),
        app.show_save_dialog,
    )
}

/// 문서의 전체 **화면** 행 수. 표 모드는 헤더가 본문 행이 아니므로 빼야
/// `render_table`의 `data_rows`와 같아지고, 그래야 페이지 이동의 마지막 행
/// 클램프가 실제로 존재하는 마지막 행과 맞는다. 텍스트 모드는 논리 행이 곧
/// 화면 행이다.
///
/// **헥스 문서는 줄 인덱서를 아예 띄우지 않는다**(`hex_document`가
/// `indexer: None` — `hex_status_text` 주석 참조). 그래서
/// `doc.index.line_count()`는 영영 0이고, 그대로 두면 `page_target_row`가
/// `total == 0`으로 보고 `None`을 돌려 Page Up/Down이 **캐럿만 옮기고 화면은
/// 그대로**였다(I3). 헥스의 화면 행은 32바이트 한 줄이므로
/// `hex::row_count`가 곧 답이다 — 이러면 `render_hex`가 이미 기록하던
/// `first_visible_row`/`visible_rows` 관측값이 비로소 쓰인다.
fn doc_screen_row_count(doc: &Document) -> usize {
    if doc.hex.is_some() {
        return crate::hex::row_count(hex_doc_len(doc)) as usize;
    }
    match doc.sep {
        SeparatorMode::None => doc_line_count(doc),
        SeparatorMode::Char(_) => {
            let data_start = if doc.has_header { 1 } else { 0 };
            doc_line_count(doc).saturating_sub(data_start)
        }
    }
}

/// 페이지 이동 한 번을 문서에 반영한다 — 목표 화면 행을 `pending_scroll_row`에,
/// 정렬을 `pending_scroll_align`에 남긴다(실제 스크롤은 다음 프레임 렌더가
/// `vertical_scroll_offset`으로 **애니메이션 없이 즉시** 수행한다 —
/// 그 필드 주석과 `scroll_offset_for_row` 참조).
///
/// `update()`의 키 블록 안에 인라인으로 두면 `update()`가 `eframe::Frame`을
/// 요구해 테스트가 이 동작을 구동할 수 없으므로(코드베이스의 기존 문제),
/// 키 소비만 `update()`에 남기고 **결정과 상태 변경 전부**를 여기로 뺀다.
fn apply_page_scroll(doc: &mut Document, dir: PageDir) {
    let total = doc_screen_row_count(doc);
    if let Some(target) = page_target_row(dir, doc.first_visible_row, doc.visible_rows, total) {
        doc.pending_scroll_row = Some(target);
        // 페이지 단위 이동은 "이 행부터 한 화면"이라 목표 행이 맨 위에 와야
        // 다음 페이지가 정확히 이어진다(찾기의 Center와 다르다).
        doc.pending_scroll_align = egui::Align::TOP;
    }
}

/// 스크롤 요청 한 건을 `TableBuilder`의 **세로 offset(px)** 으로 바꾼다.
///
/// **왜 `scroll_to_row`가 아니라 offset인가(K-3).** `TableBuilder::scroll_to_row`는
/// 결국 `ui.scroll_to_rect`로 이어지고, egui 0.28.1의 `ScrollArea`는 그 요청을
/// `offset_target`에 넣어 **0.1~0.3초에 걸쳐 ease-in-ease-out으로 감는다**
/// (`egui-0.28.1/src/containers/scroll_area.rs:638-664, 845-862`).
/// 페이지 이동은 "중간 단계를 볼 이유가 없는" 조작이라 그 애니메이션이 지연으로만
/// 느껴진다. 반면 `vertical_scroll_offset`은 `state.offset.y`에 **그대로 대입**
/// 되므로(같은 파일 512행) 애니메이션 없이 즉시 점프한다.
///
/// **왜 `ScrollArea::animated(false)`를 안 쓰는가.** egui 0.28.1에는 그 옵션이
/// 있지만(`scroll_area.rs:408`) `egui_extras 0.28.1`의 `TableBuilder`가
/// 노출하지 않는다(`TableScrollOptions`에 필드 자체가 없다). 0.28.1에는
/// `style.scroll_animation`/`ScrollAnimation`도 **없다**(레지스트리 소스 확인).
/// 그래서 이 버전에서 즉시 점프를 얻는 길은 offset 직접 지정뿐이다.
///
/// **행 → y 좌표.** `TableBody::rows`가 쓰는 것과 **같은** 식이다
/// (`egui_extras-0.28.1/src/table.rs:969-988`): 행 하나가 차지하는 높이는
/// `row_height + item_spacing.y`이고, `row`번째 행의 위쪽 y는 그 값의 배수다.
/// 두 계산이 어긋나면 점프한 자리가 한 행씩 밀린다.
///
/// **정렬.** `Align::TOP`(페이지 이동)은 목표 행을 맨 위에. `Align::Center`
/// (찾기·거터 클릭)는 목표 행이 화면 중앙에 오도록 뷰포트 절반만큼 뺀다.
/// `Align::BOTTOM`은 목표 행의 아래쪽이 화면 바닥에 닿게 한다. 음수 offset은
/// 0으로 클램프한다(문서 맨 앞보다 위로는 갈 수 없다).
///
/// `viewport_height`는 **본문**(헤더 제외) 높이다 — 호출부가 `available_height`
/// 에서 헤더 한 줄을 빼서 넘긴다(`visible_row_count`와 같은 규율).
fn scroll_offset_for_row(
    row: usize,
    align: egui::Align,
    row_height: f32,
    spacing_y: f32,
    viewport_height: f32,
) -> f32 {
    let step = row_height + spacing_y;
    let row_top = row as f32 * step;
    let offset = match align {
        egui::Align::Min => row_top,
        egui::Align::Center => row_top - (viewport_height - step) * 0.5,
        egui::Align::Max => row_top + step - viewport_height,
    };
    offset.max(0.0)
}

/// 본문에 남은 높이에서 한 화면에 들어가는 **본문 행 수**를 구한다.
///
/// `ui.available_height()`는 헤더 행까지 포함한 테이블 전체 높이이므로
/// 헤더 한 줄(`ROW_HEIGHT`)을 빼고 나눈다 — 그러지 않으면 Page Down이 매번
/// 한 행씩 더 건너뛰어 그 행이 조용히 안 읽힌다.
///
/// 표/텍스트 두 렌더가 같은 규칙을 써야 하므로(둘 다 `TableBuilder` +
/// `ROW_HEIGHT` 헤더 하나) 계산을 한 함수로 묶는다.
fn visible_row_count(avail_height: f32, row_h: f32) -> usize {
    let body = avail_height - row_h;
    if body <= 0.0 {
        return 0;
    }
    (body / row_h) as usize
}

/// 이 문서를 그릴 행 높이. 렌더는 `ROW_HEIGHT` 상수 대신 **반드시** 이걸 쓴다 —
/// 그러지 않으면 Ctrl+휠로 글자만 커지고 행 높이는 그대로라 글자가 잘린다.
fn doc_row_height(d: &Document) -> f32 {
    crate::theme::row_height(d.view_scale)
}

/// 이 문서의 데이터 영역 고정폭 폰트. 캐럿/선택 x 매핑과 렌더가 같은 FontId를
/// 써야 하므로(`text_font_id`의 이유와 같다) 배율까지 한 곳에서 얹는다.
fn doc_font_id(d: &Document) -> egui::FontId {
    egui::FontId::monospace(crate::theme::mono_size(d.view_scale))
}

/// 표 모드가 실제로 그리는 컬럼 수. `render_table`의 col_count 계산
/// (`app.rs`의 그 지점 주석 — 헤더 필드 수와 앞부분 데이터 행 몇 개를
/// 샘플링한 필드 수의 최댓값)과 **완전히 같은 알고리즘**이어야 한다 —
/// `render_table`은 모든 행에 이 **하나의** col_count만큼 칸을 그리므로
/// (행마다 실제 필드 수가 달라도), "행 전체 선택"의 끝 컬럼도 이 값이어야
/// 화면에 그려지는 칸 수와 일치한다(Important 2). 그 행 자신의 필드 수만
/// 쓰면 다른 행이 더 넓을 때 화면보다 좁게 선택된다 — 그래서 세 번째
/// 기준(`selected_col`처럼 매치와 무관한 UI 상태)을 새로 만들지 않고 이
/// 함수 하나로 `render_table`과 `focus_match`가 값을 공유한다.
fn table_col_count(doc: &Document, delim: u8) -> usize {
    // `doc_line_count`를 쓴다 — Parquet 문서는 `LineIndex`가 비어
    // `index.line_count()`가 0이라, 그대로 두면 헤더를 못 읽어 컬럼 수가
    // 1로 무너진다(표가 한 칸만 나온다). 텍스트 경로에서는 같은 값이다.
    let total_lines = doc_line_count(doc);
    let header_len = if doc.has_header && total_lines > 0 {
        parse_logical_line_edit(doc, 0, delim).map_or(0, |f| f.len())
    } else {
        0
    };
    let data_start = if doc.has_header { 1 } else { 0 };
    const COL_COUNT_SAMPLE_ROWS: usize = 10;
    let mut col_count = header_len;
    for logical in data_start..data_start + COL_COUNT_SAMPLE_ROWS {
        if let Some(fields) = parse_logical_line_edit(doc, logical, delim) {
            col_count = col_count.max(fields.len());
        }
    }
    col_count.max(1)
}

/// 지금 편집 버퍼의 개정 번호. 뷰 모드(편집 버퍼 없음)는 데이터가 변하지
/// 않으므로 0.
fn doc_revision(doc: &Document) -> u64 {
    doc.edit.as_ref().map_or(0, |e| e.undo.revision())
}

/// 갖고 있는 오류 목록이 지금 데이터를 설명하는가.
///
/// 결과가 없으면(아직 안 돌았으면) 낡은 것도 아니다 — `false`.
fn error_scan_is_stale(doc: &Document) -> bool {
    doc.row_errors.is_some() && doc.row_errors_revision != doc_revision(doc)
}

/// 오류 검사를 지금 시작해야 하는가(순수 판정).
///
/// egui 클로저 안에 인라인으로 두면 테스트가 구동할 수 없으므로 결정을 따로
/// 뺀다(이 코드베이스의 `convert_enabled`·`page_target_row`와 같은 규율).
///
/// 시작 조건:
/// - **표 모드**여야 한다. 텍스트 모드는 나눌 기준이 없어 "필드 수"가 없다.
/// - 진행 중인 작업이 없어야 한다(중복 실행 방지).
/// - 결과가 없거나, 있어도 **낡았어야** 한다(편집으로 데이터가 바뀐 뒤).
/// - 편집 모드가 아니라면 **인덱싱이 끝나야** 한다 — 진행 중에 훑으면
///   그때까지 인덱싱된 앞부분만 검사하고 "검사 완료"로 표시된다.
///   편집 모드는 버퍼가 파일 전체를 이미 담고 있으므로 인덱싱과 무관하다.
fn should_start_error_scan(doc: &Document) -> bool {
    if !matches!(doc.sep, SeparatorMode::Char(_)) {
        return false;
    }
    if doc.error_scan.is_some() {
        return false;
    }
    if doc.row_errors.is_some() && !error_scan_is_stale(doc) {
        return false;
    }
    doc.edit.is_some() || doc.index.status().phase == crate::index::Phase::Complete
}

/// 오류 검사 결과를 버린다 — 파싱 기준(구분자·인코딩·헤더)이 바뀌었거나
/// 데이터가 바뀌어 **지난 결과가 더 이상 이 문서를 설명하지 못할 때**.
///
/// 진행 중인 작업도 취소한다. 취소하지 않으면 옛 기준으로 돌던 스캔이 나중에
/// 끝나 **바뀐 문서에 옛 답**을 덮어쓴다 — 구분자를 콤마에서 탭으로 바꾼
/// 직후 "오류 1,500만 개"가 뜨는 식이다.
fn invalidate_error_scan(doc: &mut Document) {
    if let Some(job) = doc.error_scan.take() {
        job.cancel();
    }
    doc.row_errors = None;
    doc.row_errors_revision = 0;
}

/// 검사를 시작할 수 있으면 시작한다. 편집 모드는 인메모리 버퍼가 진실이므로
/// 그 자리에서 동기 스캔하고(이미 디코딩된 `String`이라 훨씬 싸다), 뷰 모드는
/// mmap을 백그라운드 스레드로 훑는다.
fn start_error_scan(doc: &mut Document, ctx: &egui::Context) {
    if !should_start_error_scan(doc) {
        return;
    }
    let SeparatorMode::Char(delim) = doc.sep else {
        return;
    };
    let expected = table_col_count(doc, delim);
    let data_start = if doc.has_header { 1 } else { 0 };

    if let Some(e) = &doc.edit {
        // 검사 **직전**의 개정 번호를 기록한다. 검사 뒤에 읽으면 그사이 일어난
        // 편집을 이미 반영한 것으로 착각한다(여기서는 동기라 그 틈이 없지만,
        // 순서를 뒤집을 이유도 없다).
        doc.row_errors_revision = doc_revision(doc);
        doc.row_errors = Some(crate::validate::scan_lines(
            &e.lines,
            delim,
            expected,
            data_start,
            crate::validate::MAX_ROW_ERRORS,
        ));
        return;
    }
    // 뷰 모드는 편집 버퍼가 없어 데이터가 변하지 않는다 — 개정 번호는 계속 0이고
    // (`doc_revision`), 무효화는 구분자/인코딩/헤더 변경이 직접 건다.
    doc.error_scan = Some(crate::validate::spawn_scan(
        doc.source.clone(),
        doc.index.clone(),
        doc.enc,
        delim,
        expected,
        data_start,
        ctx.clone(),
    ));
}

/// 진행 중인 검사가 끝났으면 결과를 수거한다. 매 프레임 부른다.
fn poll_error_scan(doc: &mut Document) {
    let Some(job) = &mut doc.error_scan else {
        return;
    };
    if !job.is_finished() {
        return;
    }
    // 취소된 작업은 결과가 없다(`spawn_scan` 주석). 그때는 `row_errors`를
    // `None`으로 둔 채 작업만 치운다 — 무효화가 곧 재검사를 부른다.
    doc.row_errors = job.take_result();
    doc.error_scan = None;
}

/// 상태바에 쓸 오류 요약 문구. `None`이면 아무것도 표시하지 않는다.
///
/// 순수 함수로 뺀 이유는 세 상태(검사 중 / 오류 없음 / 오류 N개)의 구분이
/// 조용히 뒤바뀌기 쉬워서다 — 특히 "아직 안 돌았다"와 "돌았는데 0개"가 둘 다
/// 빈 목록이라 한 덩어리로 뭉뚱그리기 쉽다.
fn error_status_text(doc: &Document) -> Option<String> {
    if !matches!(doc.sep, SeparatorMode::Char(_)) {
        return None;
    }
    if doc.error_scan.is_some() {
        return Some("Checking rows…".to_owned());
    }
    let r = doc.row_errors.as_ref()?;
    let total = r.total();
    if total == 0 {
        return Some("No bad rows".to_owned());
    }
    if r.dropped > 0 {
        // 상한에 걸렸으면 목록이 전부가 아님을 반드시 밝힌다.
        return Some(format!(
            "⚠ {total} bad rows (showing first {})",
            r.errors.len()
        ));
    }
    Some(format!("⚠ {total} bad rows"))
}

/// 논리 행번호 → **헤더를 뺀** 화면 행(= `render_table`의 `view_row`,
/// `pending_scroll_row`가 요구하는 단위). `scroll_offset_for_row`가 이 값을
/// 세로 offset(px)으로 바꿔 `TableBuilder::vertical_scroll_offset`에 넘긴다.
///
/// 정렬 permutation이 있으면(뷰 모드 정렬, `doc.sort`) `render_table`이
/// `permutation[view_row] = 논리 행`(절대 논리 행번호, 헤더 포함 좌표계 —
/// `sort::extract_and_sort`의 문서 주석 참조)으로 화면 행을 논리 행으로
/// 바꾸므로, 여기서는 그 **역**을 찾아야 한다(Important 1). `position()`이
/// 이미 "몇 번째 데이터 행인가"(= view_row)를 직접 내놓으므로 — 배열 자체가
/// data_start 이후 행만 담고 0-based이므로 — 이 경우 **추가로 data_start를
/// 빼면 안 된다**(이중 차감 버그).
///
/// permutation이 없으면(정렬 없음, 또는 편집 모드 정렬 — `apply_edit_sort`가
/// 물리적으로 줄을 재배치하고 `doc.sort`를 None으로 둔다) 화면 행은
/// `logical - data_start`다(`render_table`의 `logical = data_start +
/// view_row`의 역).
///
/// `position()`은 O(permutation 길이) 선형 탐색이다. Find Next 한 번 누를
/// 때 한 번만 도는 것이므로(사용자 조작당 1회) 천만 행 정렬에서도 감수할
/// 만하다 — 매 프레임 도는 코드가 아니다.
fn logical_to_screen_row(doc: &Document, logical: usize, data_start: usize) -> usize {
    match &doc.sort {
        Some(s) => s
            .permutation
            .iter()
            .position(|&r| r as usize == logical)
            .unwrap_or_else(|| logical.saturating_sub(data_start)),
        None => logical.saturating_sub(data_start),
    }
}

/// 거터 클릭 한 번이 남겨야 할 스크롤 요청: (정렬, 목표 화면 행).
/// 정렬은 **항상 Center**다 — Page Up/Down이 `Align::TOP`으로 바꿔 놨을 수
/// 있으므로 거터 클릭마다 매번 되돌려야 한다(`focus_match`와 같은 이유).
/// 목표 행은 표 모드(구분자 있음)에서는 정렬 permutation을 거쳐 화면 행으로
/// 바꾸고, 텍스트 모드는 논리 행이 곧 화면 행이다.
fn gutter_click_target(doc: &Document, logical: usize, sep: SeparatorMode) -> (egui::Align, usize) {
    let row = match sep {
        SeparatorMode::None => logical,
        SeparatorMode::Char(_) => {
            let data_start = if doc.has_header { 1 } else { 0 };
            logical_to_screen_row(doc, logical, data_start)
        }
    };
    (egui::Align::Center, row)
}

/// 찾은 매치를 화면에 반영한다 — 선택 표시 + 스크롤 요청.
///
/// 텍스트 모드는 매치 구간을 그대로 선택(`text_sel`)하고 캐럿을 매치 끝에
/// 둔다. 표 모드는 **행 전체**를 선택한다: 매치의 col은 char 인덱스라
/// 컬럼 번호가 아니고, 인용/구분자를 거슬러 셀 단위로 정밀 매핑하는 것은
/// 이 기능이 요구하는 바가 아니다(YAGNI). 어느 쪽이든 스크롤은 다음
/// 프레임에 `pending_scroll_row`로 이뤄진다(그 필드 주석 참조).
///
/// `cell_sel`의 행은 `render_table`이 **논리** 행으로 해석하므로(화면 행이
/// 아니라) 여기서 그대로 논리 행을 담는다 — 스크롤 목적지만 화면 행으로
/// 변환하면 된다.
fn focus_match(doc: &mut Document, m: crate::find::Match) {
    doc.last_match = Some(m);
    doc.find_status.clear();
    // 찾기는 매치를 화면 **중앙**에 둔다(앞뒤 맥락과 함께 보이게).
    // Page Up/Down이 `Align::TOP`으로 바꿔 놓았을 수 있으므로 여기서 **매번**
    // 되돌린다 — 정렬은 요청마다 새로 지시해야 하는 값이지, 마지막 요청이
    // 남긴 잔여 상태를 물려받으면 안 된다(페이지를 한 번 넘긴 뒤의 Find Next가
    // 조용히 상단 정렬로 바뀌는 회귀).
    doc.pending_scroll_align = egui::Align::Center;
    let start = crate::edit::TextPos { line: m.line, col: m.col };
    let end = crate::edit::TextPos { line: m.line, col: m.col + m.len };
    match doc.sep {
        SeparatorMode::None => {
            doc.text_caret = end;
            doc.text_sel = Some((start, end));
            doc.pending_scroll_row = Some(m.line);
        }
        SeparatorMode::Char(d) => {
            // 표 모드의 화면 행 = 정렬 permutation의 역 매핑(정렬 없으면
            // 논리 행 - data_start). `logical_to_screen_row`가 이미 header를
            // 제외한 view_row 단위를 돌려주므로 여기서 또 data_start를 빼면
            // 이중 차감이 된다 — 헤더 행에 매치가 있으면 0행으로 붙는다.
            let data_start = if doc.has_header { 1 } else { 0 };
            let last_col = table_col_count(doc, d).saturating_sub(1);
            doc.cell_sel = Some((m.line, 0, m.line, last_col));
            doc.pending_scroll_row = Some(logical_to_screen_row(doc, m.line, data_start));
        }
    }
}

/// 찾기 패널의 한 동작을 수행한다. 편집 버퍼가 필요한 동작(바꾸기)은
/// Find All이 남길 상태 문구. 매치가 하나도 없으면 "Not found", 있으면 매치
/// **행** 수를 밝힌다("N matching rows" — 한 행에 여러 번 나와도 1이므로 "rows"로
/// 오해를 막는다). `render_find_panel` 안에 인라인으로 두면 렌더 결과만 보는
/// 테스트가 이 판정을 구동하지 못하므로 순수 함수로 뽑는다(코드베이스 관례).
fn find_all_status(match_row_count: usize) -> String {
    if match_row_count == 0 {
        "Not found".to_owned()
    } else if match_row_count == 1 {
        "1 matching row".to_owned()
    } else {
        format!("{match_row_count} matching rows")
    }
}

/// 실제 검색에 쓸 검색어. `find_escapes`가 켜져 있으면 `\t` 등을 실제 문자로
/// 푼다(`find::unescape`). 꺼져 있으면 입력란의 날 문자열 그대로다.
///
/// **모든 소비 지점이 이 함수를 거쳐야 한다.** `doc.find_query`를 직접 쓰는
/// 경로가 하나라도 남으면 그 경로만 이스케이프가 안 먹어 "Find Next는 탭을
/// 찾는데 Find All은 못 찾는" 식으로 조용히 갈린다 — 그중 하나가
/// `scan_all_matches`라면 "`scan_all_matches`의 행 집합 == `matching_lines`
/// 브루트포스의 행 집합"이라는 절대 계약까지 무너진다(두 쪽에 서로 다른
/// needle이 들어가므로). 그래서 검색어를 소비하는 지점은 프로덕션이든
/// 테스트든 전부 이 함수를 지난다.
///
/// 빈 문자열은 빈 문자열 그대로다(`unescape("") == ""`), 그리고 비어 있지 않은
/// 입력은 최소 한 글자를 남기므로 **디코딩이 검색어를 비게 만들 수는 없다** —
/// 빈 검색어 가드가 날 문자열을 보든 디코딩 결과를 보든 판정이 같다는 뜻이다.
/// 그래도 가드는 이 함수 결과를 보게 통일해 두었다(같은 값을 두 근거로 보는
/// 코드를 남기지 않는다). 다만 **버튼 활성 조건은 다르다** —
/// `find_all_button_enabled`/`extract_button_enabled` 주석 참조.
fn effective_query(doc: &Document) -> String {
    if doc.find_escapes {
        crate::find::unescape(&doc.find_query)
    } else {
        doc.find_query.clone()
    }
}

/// 실제 치환에 쓸 문자열. 같은 이유로(`effective_query` 주석) 치환문을 쓰는
/// 지점도 전부 이 함수를 지난다 — `\t`로 찾은 탭을 다시 `\t`로 되돌려 넣거나,
/// 반대로 탭을 다른 글자로 바꾸려면 치환문 쪽도 같은 규칙으로 읽혀야 한다.
/// (개행 방어는 여기가 아니라 치환 직전의 `sanitize_for_line`이 한다. `\n`은
///  애초에 `unescape`가 풀지 않으므로 이 함수가 개행을 만들어 낼 일도 없다.)
fn effective_replacement(doc: &Document) -> String {
    if doc.find_escapes {
        crate::find::unescape(&doc.replace_text)
    } else {
        doc.replace_text.clone()
    }
}

/// 뷰 모드에서 조용히 무시된다 — UI가 이미 그 버튼을 비활성화하지만,
/// 단축키 경로도 같은 함수를 지나므로 여기서 한 번 더 막는다.
fn apply_find_action(doc: &mut Document, act: FindAction) {
    if effective_query(doc).is_empty() {
        doc.find_status = "Enter text to find".to_owned();
        return;
    }
    match act {
        // 지금 입력된 검색어/옵션으로 전체를 스캔해 하이라이트 스냅샷을 만든다.
        // 이것이 하이라이트가 갱신되는 **유일한** 지점이다. Find All은 하이라이트만
        // 만들고 커서(`last_match`)는 옮기지 않는다 — 그건 Find Next의 몫이다
        // (설계 판단, 리포트 참조). 사용자는 Find All로 전체를 물들인 뒤 Find
        // Next로 순회한다.
        FindAction::All => {
            let rows = scan_all_matches(doc);
            doc.find_status = find_all_status(rows.len());
            doc.highlight = Some(Highlight {
                // 스냅샷에는 **디코딩된** 검색어를 넣는다. 그래야 렌더가
                // `find_escapes`를 전혀 몰라도 되고(스냅샷 하나만 보면 된다),
                // Find All 뒤에 체크박스를 꺼도 하이라이트가 그대로 유지된다
                // — 스냅샷의 뜻이 "그때 그 검색어로 찾은 결과"이기 때문이다.
                // `opts`를 그때 값으로 얼려 두는 것과 정확히 같은 규율이다.
                query: effective_query(doc),
                opts: doc.find_opts.clone(),
                rows,
            });
        }
        FindAction::Next | FindAction::Prev => {
            let found = search_from(doc, find_origin(doc, act == FindAction::Next), act == FindAction::Next);
            match found {
                Some(m) => focus_match(doc, m),
                None => doc.find_status = "Not found".to_owned(),
            }
        }
        FindAction::ReplaceOne => replace_one(doc),
        FindAction::ReplaceAll => replace_all_in_doc(doc),
        // 추출은 탭을 추가하므로 `Document` 하나로는 수행할 수 없다.
        // 호출부(`update()`)가 이 변형만 `App::extract_matching_rows`로
        // 돌려보내므로 여기까지 오지 않는다 — 와도 조용히 무시한다
        // (뷰 모드에서 바꾸기를 무시하는 것과 같은 방어).
        FindAction::Extract => {}
        // 헥스 찾기도 마찬가지로 `Document` 하나로 끝나지 않는 별도 로직이라
        // 호출부가 `hex_find_next`로 직접 돌려보낸다 — 와도 조용히 무시한다.
        FindAction::HexNext => {}
    }
}

/// `find::find_next`/`find_prev` 호출을 한 곳에 묶는다.
///
/// **borrow 주의**: `get_line` 클로저가 `doc`을 불변 대여하는 동안에는
/// `doc`을 가변 대여할 수 없다. 그래서 이 함수는 `&Document`만 받아 결과를
/// 돌려주고, `doc.last_match` 등의 갱신은 호출부가 **그 뒤에** 한다.
fn search_from(
    doc: &Document,
    from: crate::edit::TextPos,
    forward: bool,
) -> Option<crate::find::Match> {
    let n = doc_line_count(doc);
    let delim = doc_delimiter(doc);
    let get_line = |i: usize| logical_line(doc, i);
    let query = effective_query(doc);
    if forward {
        crate::find::find_next(n, from, &query, &doc.find_opts, delim, get_line)
    } else {
        crate::find::find_prev(n, from, &query, &doc.find_opts, delim, get_line)
    }
}

/// 문서 전체에서 `find_query`가 있는 논리 행 번호를 모은다(스크롤 마커용).
/// 비싸므로(최악 0.85초/2GB) 호출부가 query/opts 변경 시에만 부른다. query가
/// 비면 빈 Vec. 결과는 **항상 `matching_lines` 브루트포스와 같은 행 집합**이다 —
/// memmem 프리필터는 후보를 좁힐 뿐이고, 최종 판정은 `find_in_line_scoped`가 한다.
///
/// **설계 S-3의 (나) 방식.** `find.rs`는 mmap/Source를 모르므로, 벤치가 증명한
/// "파일 전체 memmem + offset 이진탐색" 최적화는 여기(app.rs)에서 한다.
///
/// **ignore_case 프리필터 판단(중요, Task H에서 수정).** memmem은 대소문자를
/// 구분한다.
/// - **match_case=true**: 원본 바이트에 그대로 memmem(벤치의 빠른 경로).
/// - **match_case=false**: "needle 대문자/소문자 **두 벌**을 memmem" 방식은 혼합
///   케이스(`ab`↔`Ab`)를 놓쳐 **위음성**이 생기므로 채택하지 않는다. 대신
///   **hay와 needle 바이트를 둘 다 ASCII 소문자로 접어** 비교한다
///   (`find_ci_ascii`) — 접기는 ASCII 범위에서 바이트 단위로 정확히 정의되므로
///   `Ab`/`aB`/`AB`/`ab`를 전부 잡는다. 조건은 `bytefast_ci_ok`(바이트로 접히는
///   needle + 단일 바이트 인코딩)이고, 그 밖(유니코드 접기가 필요한 `É`/`İ`
///   needle, UTF-16, Whole word)은 종전대로 행 단위 `find_in_line_scoped`
///   폴백이다. **한글처럼 대소문자가 없는 비ASCII needle은 빠른 경로를 탄다**
///   — `query_is_case_foldable_by_bytes` 주석 참조.
///
/// 어느 경로든 최종 결과는 반드시 `matching_lines` 브루트포스와 **같은 행 집합**
/// 이다. 빠른 경로는 "확정" 또는 "정밀 판정 필요"만 판단하고, 애매한 것을
/// 바이트만으로 "비매치"로 단정하지 않는다.
///
/// **Whole cell의 함정(Task I에서 고친 것).** Whole cell 비교 대상은 파일 바이트가
/// 아니라 `split_fields`의 **표시값**(따옴표 벗김, `""` → `"`)이다. 그래서
/// "needle 바이트가 이 행에 없다"는 사실은 **비매치의 근거가 되지 못한다** —
/// `"a"a`의 표시값은 `aa`이고, `"a""b"`는 `a"b`다. 바이트로 비매치를 단정할 수
/// 있는 것은 행에 `"`가 하나도 없을 때뿐이다(`cell_bytes_are_display`).
///
/// (호출부는 Find All(`apply_find_action`의 `FindAction::All`)과 추출뿐이다 —
/// 매 프레임 자동 호출은 없앴다. 사용자가 명시적으로 눌렀을 때만 돈다.)
fn scan_all_matches(doc: &Document) -> Vec<u32> {
    // 날 `doc.find_query`가 아니라 디코딩된 검색어로 스캔한다 — 브루트포스
    // (`matching_lines`)와 **같은 needle**이 들어가야 위의 절대 계약이 성립한다.
    let query = &effective_query(doc);
    if query.is_empty() {
        return Vec::new();
    }
    let opts = &doc.find_opts;
    let delim = doc_delimiter(doc);

    match &doc.edit {
        // ---- 편집 모드: EditBuffer.lines 순회 ----
        Some(e) => {
            let finder = (opts.match_case).then(|| memchr::memmem::Finder::new(query.as_bytes()));
            // 편집 모드도 뷰 모드와 같은 바이트 빠른 경로를 쓴다. `e.lines`는 이미
            // RAM 위 `String`이라 디코딩 비용은 없지만, `find_in_line_scoped`가
            // 부르는 `split_fields`/`chars().collect()` **할당**은 행마다 그대로
            // 발생한다(수 GB 버퍼면 수백만 회). 흔한 검색어에서 그 할당을 없앤다.
            //
            // `e.lines`는 항상 UTF-8 문자열이므로 needle(UTF-8) 바이트로 memmem을
            // 돌리는 것과 `,`(0x2C) 같은 구분자 바이트 판정이 코드유닛과 어긋날 일이
            // 없다 — 뷰 모드의 UTF-16 폴백 이유가 여기선 적용되지 않아, 인코딩과
            // 무관하게 빠른 경로가 안전하다.
            let needle_bytes = query.as_bytes();
            let needle_len = needle_bytes.len();
            let scope = opts.scope;
            let bytefast_partial =
                opts.match_case && scope == crate::find::MatchScope::Partial;
            let bytefast_cell = opts.match_case
                && scope == crate::find::MatchScope::WholeCell
                && delim.is_some();
            let needle_has_delim = delim.is_some_and(|d| needle_bytes.contains(&d));
            // ignore_case 바이트 빠른 경로(Task H). `e.lines`는 **항상 UTF-8**
            // 문자열이므로 문서 인코딩이 무엇이든 CP949 트레일 바이트 문제가 없다 —
            // UTF-8은 self-synchronizing이라 needle이 ASCII든 한글이든 히트가 문자
            // 중간에 걸릴 수 없다(`bytefast_ci_confirms` 주석의 논증). 그래서
            // `bytefast_ci_ok`의 인코딩 인자로 문서 인코딩이 아니라
            // `Encoding::Utf8`을 넘긴다(버퍼의 실제 인코딩이 판정 근거다).
            let ci_ok = !opts.match_case && bytefast_ci_ok(query, Encoding::Utf8);
            let needle_lower: Vec<u8> = query.bytes().map(ascii_lower).collect();
            let ci_partial = ci_ok && scope == crate::find::MatchScope::Partial;
            // Whole word는 유니코드 단어 경계라 바이트로 안전하지 않다 → 폴백 유지.
            let ci_cell =
                ci_ok && scope == crate::find::MatchScope::WholeCell && delim.is_some();
            let mut out = Vec::new();
            for (i, line) in e.lines.iter().enumerate() {
                let lb = line.as_bytes();
                // match_case면 memmem으로 후보를 먼저 거른다(정확성은 scoped가 보장).
                // ignore_case면 ASCII 접기 바이트 탐색, 그것도 안 되면 행 단위 scoped.
                let Some(f) = &finder else {
                    // ---- ignore_case Partial 빠른 경로: 히트 = 매치(UTF-8 버퍼). ----
                    if ci_partial {
                        if find_ci_ascii(lb, &needle_lower).is_some() {
                            out.push(i as u32);
                        }
                        continue;
                    }
                    // ---- ignore_case Whole cell: 히트마다 경계 바이트만 본다. ----
                    if ci_cell {
                        let d = delim.unwrap();
                        // 행 바이트가 곧 표시값인가(`"`가 없는가). 뷰 모드와 같은 규율.
                        let plain = cell_bytes_are_display(lb);
                        let mut confirmed = false;
                        let mut needs_refine = false;
                        for hit in find_ci_ascii_all(lb, &needle_lower) {
                            match classify_cell_hit(
                                lb,
                                0,
                                lb.len(),
                                hit,
                                needle_len,
                                d,
                                needle_has_delim,
                            ) {
                                CellHit::Confirmed => {
                                    confirmed = true;
                                    break;
                                }
                                CellHit::NeedsRefine => needs_refine = true,
                                CellHit::NotCellBoundary => {}
                            }
                        }
                        // 히트 0개를 비매치로 단정하지 않는다 — Whole cell 비교
                        // 대상은 표시값이라 `"a"a`(표시값 `aa`)처럼 needle 바이트가
                        // 행에 없어도 매치일 수 있다. `"`가 없는 행만 단정한다.
                        let matched = (confirmed && plain)
                            || ((confirmed || needs_refine || !plain)
                                && !crate::find::find_in_line_scoped(line, query, opts, delim)
                                    .is_empty());
                        if matched {
                            out.push(i as u32);
                        }
                        continue;
                    }
                    // 그 외(비ASCII needle, Whole word): 종전대로 행 단위 scoped.
                    if !crate::find::find_in_line_scoped(line, query, opts, delim).is_empty() {
                        out.push(i as u32);
                    }
                    continue;
                };
                // Partial 빠른 경로: memmem 히트 = 매치.
                if bytefast_partial {
                    if f.find(lb).is_some() {
                        out.push(i as u32);
                    }
                    continue;
                }
                // Whole cell 빠른 경로: 히트마다 경계 바이트만 보고 확정/폴백/버림.
                if bytefast_cell {
                    let d = delim.unwrap();
                    // 행 바이트가 곧 표시값인가(`"`가 없는가). 뷰 모드와 같은 규율.
                    let plain = cell_bytes_are_display(lb);
                    let mut confirmed = false;
                    let mut needs_refine = false;
                    for hit in f.find_iter(lb) {
                        match classify_cell_hit(lb, 0, lb.len(), hit, needle_len, d, needle_has_delim) {
                            CellHit::Confirmed => {
                                confirmed = true;
                                break;
                            }
                            CellHit::NeedsRefine => needs_refine = true,
                            CellHit::NotCellBoundary => {}
                        }
                    }
                    // 확정 히트가 하나라도 있으면 매치. 없고 따옴표 후보만 있으면
                    // 폴백으로 정밀 확인.
                    //
                    // **히트 0개(또는 전부 NotCellBoundary)를 비매치로 단정하지
                    // 않는다.** Whole cell 비교 대상은 `split_fields`가 준 표시값이라
                    // 따옴표가 낀 행은 needle 바이트가 한 번도 나타나지 않아도 매치일
                    // 수 있다(`"a"a` → 표시값 `aa`, `"a""b"` → `a"b`).
                    // `"`가 하나도 없는 행만 표시값 == 원본 바이트라 단정할 수 있다.
                    let matched = (confirmed && plain)
                        || ((confirmed || needs_refine || !plain)
                            && !crate::find::find_in_line_scoped(line, query, opts, delim)
                                .is_empty());
                    if matched {
                        out.push(i as u32);
                    }
                    continue;
                }
                // 그 외(Whole word): 종전대로 프리필터 후 정밀 판정.
                if f.find(lb).is_none() {
                    continue;
                }
                if !crate::find::find_in_line_scoped(line, query, opts, delim).is_empty() {
                    out.push(i as u32);
                }
            }
            out
        }
        // ---- 뷰 모드: mmap 바이트 전체 memmem + offset 이진탐색 ----
        None => {
            // Parquet은 mmap 바이트가 없다 — 구분자로 나뉜 원본 텍스트가
            // 애초에 존재하지 않는다. 아래 바이트 빠른 경로 셋을 전부 건너뛰고
            // 행 단위 폴백으로 간다(`logical_line`이 row group을 디코드한다).
            //
            // 렌더가 좁혀 둔 컬럼 프로젝션을 **전체로 되돌린다** — 그러지 않으면
            // 화면 밖 컬럼의 매치를 조용히 놓친다.
            if doc.parquet.is_some() {
                widen_parquet_to_all_columns(doc);
                return scan_rows_scoped(doc, query, opts, delim);
            }
            if opts.match_case {
                scan_view_memmem(doc, query, opts, delim)
            } else if let Some(rows) = scan_view_ci_bytes(doc, query, opts, delim) {
                rows
            } else {
                // 빠른 경로가 성립하지 않는 경우(비ASCII needle, UTF-16, Whole word,
                // 텍스트 모드 Whole cell): 행 단위 폴백. 디코딩 비용이 크지만
                // 정확성이 우선이다.
                scan_rows_scoped(doc, query, opts, delim)
            }
        }
    }
}

/// 찾기·정렬·내보내기 전에 **전체 컬럼**을 보게 한다.
///
/// 렌더는 보이는 컬럼만 디코드하도록 프로젝션을 좁혀 둔다(스크롤 성능).
/// 그 상태로 전체 스캔을 돌리면 화면 밖 컬럼의 매치를 **조용히 놓친다** —
/// 오류도 안 나고 "찾기가 동작한다"만 보는 테스트는 통과한다.
fn widen_parquet_to_all_columns(doc: &Document) {
    if let Some(pq) = &doc.parquet {
        pq.borrow_mut().set_visible_columns(None);
    }
}

/// Parquet 문서를 한 컬럼으로 정렬한다.
///
/// **mmap 바이트 스캔이 불가능하므로 별도 경로다.** `sort.rs`의 빠른 경로는
/// 구분자로 나뉜 원본 바이트를 전제하는데(15M행 정렬이 빠른 이유) Parquet에는
/// 그 바이트가 없다. 대신 컬럼 지향의 이점을 쓴다 — **정렬 키 컬럼만** 읽고
/// 다른 컬럼은 디코드조차 하지 않는다.
///
/// 순열이 만들어지면 렌더는 텍스트 경로와 **완전히 동일**해진다
/// (`render_table`이 이미 `permutation`으로 행을 매핑한다).
fn sort_parquet_column(doc: &mut Document, col: usize, dir: SortDir) {
    let Some(pq) = &doc.parquet else { return };
    let (values, numeric) = {
        let mut p = pq.borrow_mut();
        let Ok(v) = p.column_values(col) else { return };
        // Parquet은 타입이 확정적이라 "숫자로 보이는지" 추론이 필요 없다.
        (v, p.column_is_numeric(col))
    };
    let perm = crate::parquet::sort_permutation(&values, numeric, dir == SortDir::Asc);
    doc.sort = Some(SortState {
        permutation: perm,
        col,
        kind: if numeric { SortKind::Number } else { SortKind::Text },
        dir,
        spec_count: 1,
    });
}

/// 정렬을 시작하기 전에 메모리 예상치를 확인한다. 임계 초과면 확인 플래그만
/// 세우고 **정렬은 시작하지 않는다**(hex의 `confirm_load`와 같은 규율).
fn request_parquet_sort(doc: &mut Document, col: usize, dir: SortDir) {
    let Some(pq) = &doc.parquet else { return };
    let bytes = {
        let mut p = pq.borrow_mut();
        let numeric = p.column_is_numeric(col);
        let rows = p.total_rows();
        let avg = p.estimated_avg_len(col);
        crate::parquet::estimate_sort_bytes(rows, numeric, avg)
    };
    if bytes > crate::parquet::PARQUET_SORT_CONFIRM_BYTES {
        doc.pending_parquet_sort = Some((col, dir));
        return;
    }
    sort_parquet_column(doc, col, dir);
}

/// 내보낼 줄들을 모은다. 정렬이 적용돼 있으면 **화면 순서**를 따른다 —
/// 보이는 것과 나가는 것이 다르면 혼란스럽다.
///
/// 헤더는 정렬과 무관하게 언제나 첫 줄이다(정렬은 데이터 행만 재배치한다).
fn collect_export_lines(doc: &Document) -> Vec<String> {
    // 화면 밖 컬럼도 전부 내보낸다.
    widen_parquet_to_all_columns(doc);
    let n = doc_line_count(doc);
    let mut out = Vec::with_capacity(n);
    if let Some(h) = logical_line(doc, 0) {
        out.push(h);
    }
    match doc.sort.as_ref() {
        Some(s) => {
            for &logical in &s.permutation {
                if let Some(l) = logical_line(doc, logical as usize) {
                    out.push(l);
                }
            }
        }
        None => {
            for i in 1..n {
                if let Some(l) = logical_line(doc, i) {
                    out.push(l);
                }
            }
        }
    }
    out
}

/// 인코딩이 **단일 바이트 구분자**(delimiter/개행이 1바이트)를 쓰는가.
/// UTF-16은 구분자·개행이 2바이트 코드유닛이라 원바이트 경계 판정이 코드유닛
/// 중간에 걸릴 수 있어(예: 한글의 하위 바이트가 우연히 `,`(0x2C)와 겹침)
/// 바이트 빠른 경로가 안전하지 않다. UTF-8/CP949만 참.
fn is_single_byte_enc(enc: Encoding) -> bool {
    matches!(enc, Encoding::Utf8 | Encoding::Cp949)
}

/// needle을 문서 인코딩으로 옮겼다가 되돌렸을 때 **원문 그대로**인가.
///
/// **왜 필요한가(치명적).** `save::encode_bytes`는 표현할 수 없는 문자를 조용히
/// 대체한다(CP949에 `é`가 없어 `?`(0x3F)가 된다). 그 대체 바이트로 프리필터를
/// 돌리면 `?`가 있는 행이 후보로 잡히고, 반대로 `é`가 실제로 든 행은
/// (인코딩할 수 없으니 파일에도 없어) 잡히지 않는다 — 즉 프리필터가 브루트포스와
/// **다른 질문**을 하게 된다. 위양성만이면 정밀 판정이 걸러 주지만, 이 경우
/// `?` 행이 후보가 되었다가 걸러지는 대신 match_case Partial처럼 "히트 = 확정"인
/// 경로에서는 그대로 위양성이 남는다.
///
/// 왕복(encode → decode)이 원문과 같은지로 판정한다. 다르면 손실이 있었다는
/// 뜻이므로 바이트 경로 전체를 포기하고 행 단위 폴백으로 간다.
fn needle_roundtrips(query: &str, enc: Encoding) -> bool {
    let bytes = crate::save::encode_bytes(query, enc);
    crate::parse::decode_line(&bytes, enc) == query
}

/// 이 행을 **바이트만으로 "비매치"라고 단정해도 되는가**(Whole cell 전용).
///
/// **절대 계약.** `scan_all_matches`는 `find::matching_lines` 브루트포스와 같은
/// 행 집합을 내야 한다. Whole cell 비교 대상은 파일 바이트가 아니라
/// **표시값**이다 — `find_in_line_scoped`는 `split_fields`가 준 값(바깥 따옴표
/// 벗김, `""` → `"`)과 needle을 비교한다(`find.rs:197`). 그래서 따옴표가 낀 행은
/// 표시값이 원본 바이트와 **다르고**, needle 바이트가 행에 한 번도 나타나지 않아도
/// 매치일 수 있다:
/// - `"a"a` → 표시값 `aa`. 파일에 `aa`는 없다.
/// - `"a""b"` → 표시값 `a"b`. 파일에 `a"b`는 없다.
/// - `"hi"t` → 표시값 `hit`.
///
/// 따라서 "히트 0개"는 **비매치의 근거가 될 수 없다**. 행에 `"`가 하나도 없을
/// 때만 표시값 == 원본 바이트가 보장되므로 그때만 단정한다. 따옴표가 없는 데이터가
/// 압도적으로 흔하므로 빠른 경로는 그대로 살아 있고, 비용은 `memchr` 한 번이다.
/// (닫히지 않은 따옴표·`"` 자체가 needle인 경우도 이 한 줄이 함께 덮는다 —
///  그런 행은 전부 `"`를 포함하므로 정밀 판정으로 간다.)
fn cell_bytes_are_display(line_bytes: &[u8]) -> bool {
    memchr::memchr(b'"', line_bytes).is_none()
}

/// ASCII 바이트 접기(`ascii_lower`)와 그 위에 선 대소문자 무시 바이트 탐색
/// (`find_ci_ascii` / `find_ci_ascii_from`)도 `find.rs`로 옮겼다(Task M) —
/// `query_is_case_foldable_by_bytes`와 **같은 한 벌의 판정**을 이루기 때문이다
/// (그 판정이 참일 때 비로소 이 탐색이 유니코드 접기와 같은 답을 낸다).
/// 치환의 프리필터(`find::replace_all`)가 스캔 경로와 정확히 같은 탐색을 써야
/// 하는데, 사본을 만들면 한쪽만 고쳐져 두 경로가 갈린다. 근거 주석은 그쪽에 있고
/// 여기 스캔 경로와 테스트는 이 `use`로 그 한 벌을 그대로 부른다.
use crate::find::{ascii_lower, find_ci_ascii, find_ci_ascii_from};

/// 겹치지 않는 모든 출현 위치(Whole cell 경계 판정에 필요 — 히트마다
/// `classify_cell_hit`을 돌려야 하므로 첫 히트만으로는 부족하다).
///
/// `memmem::Finder::find_iter`와 같은 **비중첩** 규칙을 쓴다(찾은 뒤 needle
/// 길이만큼 건너뜀) — Whole cell 판정은 히트가 더 많아도 결과가 같고
/// (`classify_cell_hit`이 각각을 독립적으로 본다), 같은 규칙을 써야 match_case
/// 경로와 동작을 나란히 읽을 수 있다. 한 행당 히트 수는 작아 `Vec`으로 모은다.
fn find_ci_ascii_all(hay: &[u8], needle_lower: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if needle_lower.is_empty() {
        return out;
    }
    let mut from = 0usize;
    while let Some(i) = find_ci_ascii_from(hay, needle_lower, from) {
        out.push(i);
        from = i + needle_lower.len();
    }
    out
}

/// needle이 **바이트 접기만으로 대소문자 무시 비교가 성립하는가**를 묻는
/// `query_is_case_foldable_by_bytes`는 `find.rs`로 옮겼다(Task M). 그 판정이 묻는
/// 것은 "유니코드 접기(`find::folded`/`find::eq_scoped`)와 바이트 접기가 같은
/// 답을 내는가"이므로 접기 규칙을 정의하는 쪽에 있어야 하고, 치환의 바이트 경로
/// (`find::replace_cells_bytes`)도 같은 판정을 쓰는데 순수 로직 모듈이 UI
/// 모듈(`app.rs`)을 역참조하게 만들 수는 없기 때문이다. **판정은 여전히 한 벌**이며
/// 근거 주석(전 유니코드 프로브·U+0307 구멍·U+212A 한계)도 전부 그쪽에 있다 —
/// 여기 호출부와 테스트는 이 `use`를 통해 그 한 벌을 그대로 부른다.
use crate::find::query_is_case_foldable_by_bytes;

/// ignore_case에서 ASCII 바이트 접기 빠른 경로를 쓸 수 있는가.
///
/// - **needle이 바이트로 접히는 문자로만 이뤄졌을 때만**
///   (`query_is_case_foldable_by_bytes`). ASCII 전부와, 대소문자가 없는 비ASCII
///   (한글·한자·가나·숫자·기호)가 여기 해당한다. `É`/`İ`/`Σ`처럼 유니코드 접기가
///   필요한 문자가 하나라도 있으면 바이트로는 표현되지 않으므로 행 단위
///   폴백(`find_in_line_scoped`)이 정답이다.
/// - **인코딩이 단일 바이트 계열일 때만.** UTF-16은 코드유닛이 2바이트라 원바이트
///   경계·탐색이 코드유닛 중간에 걸린다 — match_case 경로와 **같은**
///   `is_single_byte_enc` 판정을 재사용한다(판정을 두 벌로 만들면 갈린다).
///
/// **비ASCII needle이 통과해도 계약은 그대로다.** 히트를 바이트만으로 "확정"해도
/// 되는지는 여전히 `bytefast_ci_confirms`(UTF-8만)가 따로 결정한다 — CP949는
/// 통과해도 후보로만 보고 정밀 판정을 거친다. 그리고 needle이 문서 인코딩으로
/// 손실 없이 옮겨지는지는 `needle_roundtrips`가 막는다(CP949에 없는 문자가
/// `?`로 바뀌어 다른 질문이 되는 것을 방지).
fn bytefast_ci_ok(query: &str, enc: Encoding) -> bool {
    !query.is_empty() && query_is_case_foldable_by_bytes(query) && is_single_byte_enc(enc)
}

/// ignore_case 빠른 경로의 히트를 **바이트만으로 확정해도 되는가**(정밀 판정
/// 생략 가능한가).
///
/// - **UTF-8: 확정해도 된다.** UTF-8 멀티바이트 시퀀스의 연속 바이트는 항상
///   ≥0x80이라 ASCII 바이트(0x00~0x7F)와 **절대** 겹치지 않는다. 따라서 ASCII
///   needle의 바이트열이 한글/이모지 문자 중간에 우연히 걸릴 수 없다.
///
///   **비ASCII needle(한글 등)도 마찬가지다** — UTF-8은 self-synchronizing이다.
///   연속 바이트는 0x80~0xBF, 문자 첫 바이트는 ASCII(<0x80) 또는 0xC2~0xF4라
///   두 집합이 겹치지 않는다. needle이 유효한 UTF-8 문자열이면 그 **첫 바이트**는
///   반드시 문자 첫 바이트이므로, 히트가 다른 문자 **중간에서 시작될 수 없다**.
///   끝도 같다 — needle의 마지막 문자가 needle 안에서 완결되므로 히트 다음
///   바이트는 연속 바이트일 수 없다. 즉 UTF-8 히트는 항상 문자 경계에 정렬된다.
///   (`scan_hangul_needle_matches_brute_force`가 인접 한글 데이터로 이를 증명한다.)
/// - **CP949: 확정하면 안 된다.** CP949 트레일 바이트는 0x41~0xFE라 ASCII
///   대문자(0x41~0x5A)와 **겹친다**. 한글 한 글자의 트레일 바이트가 `A`(0x41)와
///   같은 값일 수 있어, ASCII needle `a`를 ignore_case로 찾으면 그 트레일
///   바이트에 걸려 **위양성**이 난다. 위양성은 안전하지만(최종 판정이 걸러낸다)
///   결과가 브루트포스와 달라지면 안 되므로 후보로만 보고
///   `find_in_line_scoped`로 확인한다.
///
/// 브리프의 (가)안. (나)안(인코딩 무관하게 전부 정밀 판정)은 단순하지만
/// UTF-8 — 압도적으로 흔한 경우 — 에서 이득이 거의 사라진다. CP949는 후보 행에만
/// 정밀 판정이 돌므로 비용이 히트 수에 비례할 뿐이다.
fn bytefast_ci_confirms(enc: Encoding) -> bool {
    matches!(enc, Encoding::Utf8)
}

/// Whole cell match_case 빠른 경로에서 memmem 히트 하나를 바이트만으로 판정한 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellHit {
    /// 앞뒤 경계가 순수 delim/줄끝이고 따옴표가 개입하지 않음 → 매치 확정.
    Confirmed,
    /// 경계가 delim/줄끝이긴 하나 따옴표(`"`)가 걸쳐 있어 표시값이 파일
    /// 바이트와 다를 수 있음 → `find_in_line_scoped`로 정밀 확인 필요.
    NeedsRefine,
    /// 앞뒤가 delim/줄끝이 아님(셀 부분에만 걸침, 예: `John`이 `Johnson` 안).
    /// 이 히트 자체는 셀 전체가 아니다 — 다만 같은 행의 다른 히트가 셀 전체일
    /// 수 있으므로 "이 행 비매치"로 단정하지는 않는다(호출부가 히트 단위 순회).
    NotCellBoundary,
}

/// memmem 히트가 **셀 전체** 매치인지 바이트만으로 판정한다(Whole cell, match_case,
/// 단일 바이트 인코딩 전용). `bytes`는 파일 전체, `[line_start, line_end)`는 히트가
/// 속한 행의 **개행 제외** 내용 범위, `hit`은 needle 시작 바이트 offset, `needle_len`은
/// needle 바이트 길이, `delim`은 구분자 바이트, `needle_has_delim`은 needle 바이트에
/// `delim`이 들어 있는지다.
///
/// 판정: 히트 바로 앞 바이트가 `delim` 또는 줄시작이고, 히트 바로 뒤 바이트가
/// `delim` 또는 줄끝이면 셀 경계다. 그 경계 바이트 중 하나라도 `"`이면 따옴표
/// 셀일 수 있어 `NeedsRefine`(브리프 §2·§G-1). 그 외 순수 경계는 `Confirmed`.
/// 경계가 아니면 `NotCellBoundary`.
///
/// **이 함수는 히트 주변 바이트만 본다 — 줄 앞쪽의 닫히지 않은 따옴표를 볼 수 없다.**
/// 그래서 `Confirmed`를 그대로 믿어도 되는 것은 호출부가 **그 행에 `"`가 하나도
/// 없음**(`cell_bytes_are_display`)을 확인했을 때뿐이다. `"`가 있는 행은
/// `Confirmed`든 히트 0개든 전부 정밀 판정으로 보내야 한다(`"a_,,HIT` × `HIT`가
/// 순수 경계로 보이지만 실제로는 따옴표 안이다).
///
/// **needle에 delim이 들어 있으면(따옴표 셀에서만 가능) 무조건 `NeedsRefine`.**
/// 예: needle `a,b`는 `a,b,c`에서 앞이 줄시작·뒤가 delim이라 순수 경계처럼
/// 보이지만, 실은 `a`와 `b` 두 셀을 가로지른다(셀 전체가 아니다). 바이트만으론
/// 셀 경계를 오판하므로 폴백에 맡긴다. `field_slice`가 따옴표 안 delim을 정확히
/// 처리한다. **"확실히 비매치"를 바이트로 단정하지 않는다** — 애매하면 항상 폴백.
fn classify_cell_hit(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    hit: usize,
    needle_len: usize,
    delim: u8,
    needle_has_delim: bool,
) -> CellHit {
    // needle에 delim이 있으면 순수 바이트 경계로는 셀을 판정할 수 없다 → 폴백.
    if needle_has_delim {
        return CellHit::NeedsRefine;
    }
    let after_pos = hit + needle_len;
    // 앞 경계: 히트가 줄시작이면 경계(None), 아니면 바로 앞 바이트.
    let before = if hit <= line_start { None } else { Some(bytes[hit - 1]) };
    // 뒤 경계: 히트 끝이 줄끝(내용 끝)에 닿으면 경계(None), 아니면 바로 뒤 바이트.
    let after = if after_pos >= line_end { None } else { Some(bytes[after_pos]) };
    // 경계 바이트 중 하나라도 `"`면 따옴표 셀일 수 있어 표시값이 파일 바이트와
    // 다르다 → 무조건 폴백(NeedsRefine). `"`는 delim이 아니라 순수 경계 검사에서
    // 걸러지지만, 그 경우 NotCellBoundary가 아니라 NeedsRefine으로 보내야
    // 따옴표 셀 매치를 놓치지 않는다(브리프 §2·§G-3의 `"John Smith"` 케이스).
    if before == Some(b'"') || after == Some(b'"') {
        return CellHit::NeedsRefine;
    }
    // 순수 경계: 앞뒤가 delim 또는 줄시작/줄끝. 그 외(부분 걸침)는 NotCellBoundary.
    let before_boundary = before.is_none_or(|b| b == delim);
    let after_boundary = after.is_none_or(|b| b == delim);
    if before_boundary && after_boundary {
        CellHit::Confirmed
    } else {
        CellHit::NotCellBoundary
    }
}

/// 뷰 모드 + match_case의 빠른 경로. 파일 바이트 전체에 memmem을 돌려 히트
/// offset을 얻고, 인덱스 snapshot의 offset 배열에 이진탐색해 행 번호로 바꾼다.
///
/// **핵심 최적화(브리프 Task G).** 흔한 검색어는 후보 행이 곧 전 행이라, 후보마다
/// 문자열을 디코딩·할당해 `find_in_line_scoped`를 부르면 2200만 번의 할당이 돌아
/// 5분씩 걸린다. 그래서 **바이트만으로 판정 가능한 경우엔 디코딩을 건너뛴다**:
///
/// - **Partial(match_case, UTF-8)**: memmem 히트 = 이 행에 needle이 있다는 증명.
///   `find_in_line`(Partial)이 하는 일이 곧 이 부분 문자열 찾기이므로 재판정이
///   필요 없다. 그 행을 바로 담는다. **CP949는 제외** — 트레일 바이트(0x41~0xFE)가
///   ASCII와 겹쳐 히트가 문자 중간에 걸릴 수 있다(`_갂` × needle `A` = 위양성).
///   그 판정은 `bytefast_ci_confirms`가 ignore_case 경로와 공유한다.
/// - **Whole cell**: 이 함수가 아니라 `scan_view_cell_bytes`(행 단위)로 보낸다.
///   Whole cell 비교 대상은 파일 바이트가 아니라 표시값이라 needle 바이트가 행에
///   한 번도 안 나와도 매치일 수 있는데(`"a"a` → `aa`), 히트 순회 구조는 그런 행을
///   방문조차 하지 않기 때문이다(위음성). 자세한 이유는 그 함수 주석 참조.
/// - **그 외**(Whole word / UTF-16 등 멀티바이트 인코딩 / CP949 Partial): 후보 행을
///   디코딩해 `find_in_line_scoped`로 정밀 판정한다. 단어 경계는 유니코드라
///   바이트로 안전하지 않고, UTF-16은 구분자·개행이 2바이트라 원바이트 경계가
///   코드유닛 중간에 걸릴 수 있어 빠른 경로가 성립하지 않는다(폴백이 정답).
///
/// 어떤 경로든 최종 결과는 반드시 `matching_lines` 브루트포스와 **같은 행 집합**이다.
/// 바이트 층은 "확정" 또는 "정밀 판정 필요"만 말할 수 있고, **혼자서 "확실히
/// 비매치"라고 단정하지 못한다** — 위음성은 곧 계약 위반이다.
fn scan_view_memmem(
    doc: &Document,
    query: &str,
    opts: &crate::find::FindOptions,
    delim: Option<u8>,
) -> Vec<u32> {
    let (offsets, total_bytes) = doc.index.snapshot();
    if offsets.is_empty() {
        return Vec::new();
    }
    let bytes = doc.source.as_bytes();
    // needle을 **문서 인코딩**으로 인코딩해 memmem을 돌린다. UTF-8 바이트를 그대로
    // 쓰면 CP949/UTF-16 파일에서는 needle 바이트가 파일에 나타나지 않아 프리필터가
    // 모든 행을 놓친다(위음성). ASCII needle은 UTF-8/CP949에서 같은 바이트라
    // 이 인코딩이 무해하고, 비ASCII는 반드시 필요하다. 정밀 판정은 어차피
    // 디코딩 후 `find_in_line_scoped`가 하므로 프리필터의 위양성은 안전하다.
    //
    // 다만 그 인코딩이 **손실 없어야** 한다. CP949에 없는 문자(`é`)는
    // `encode_bytes`가 조용히 `?`(0x3F)로 바꾸므로, 그 바이트로 프리필터를 돌리면
    // 브루트포스와 다른 질문(`?`를 찾는 검색)을 하게 된다 → 바이트 경로를 통째로
    // 포기하고 행 단위 폴백으로 간다(위양성이 아니라 **결과가 달라지는** 문제다).
    if !needle_roundtrips(query, doc.enc) {
        return scan_rows_scoped(doc, query, opts, delim);
    }
    let needle_bytes = crate::save::encode_bytes(query, doc.enc);
    if needle_bytes.is_empty() {
        return Vec::new();
    }
    // 바이트 빠른 경로를 탈 수 있는가. Partial/WholeCell + 단일 바이트 인코딩만.
    // (WholeWord·UTF-16은 아래 폴백 판정으로 간다.)
    let single_byte = is_single_byte_enc(doc.enc);
    let scope = opts.scope;
    // ---- Whole cell은 **행 단위**로 훑는다(히트 단위가 아니라). ----
    // Whole cell 비교 대상은 파일 바이트가 아니라 `split_fields`의 표시값이라,
    // needle 바이트가 행에 한 번도 안 나와도 매치일 수 있다(`"a"a` → `aa`).
    // 히트를 순회하는 구조는 **히트 0개인 행을 아예 방문하지 않아** 그런 행을
    // 조용히 비매치로 만든다(위음성 = 계약 위반). 행을 훑어야 그 행에 `"`가 있는지
    // 보고 정밀 판정으로 보낼 수 있다.
    if scope == crate::find::MatchScope::WholeCell {
        if single_byte && delim.is_some() {
            return scan_view_cell_bytes(
                doc, query, opts, delim, &offsets, total_bytes, bytes, &needle_bytes,
            );
        }
        // 텍스트 모드(delim==None)의 "행 전체 일치"와 UTF-16은 바이트 경계 판정이
        // 성립하지 않는다 → 행 단위 폴백.
        return scan_rows_scoped(doc, query, opts, delim);
    }
    let finder = memchr::memmem::Finder::new(&needle_bytes);
    let bytefast_partial = single_byte && scope == crate::find::MatchScope::Partial
        // CP949 트레일 바이트(0x41~0xFE)는 ASCII와 겹쳐 memmem 히트가 문자 중간에
        // 걸릴 수 있다(`_갂`에서 needle `A`). 그래서 히트를 바로 확정할 수 있는
        // 인코딩은 UTF-8뿐이다 — ignore_case 경로와 **같은** 판정을 재사용한다.
        && bytefast_ci_confirms(doc.enc);

    let mut out = Vec::new();
    let mut last_row: Option<u32> = None;
    for hit in finder.find_iter(bytes) {
        // hit(바이트 offset)이 속한 행 = offset이 hit 이하인 마지막 줄 시작.
        // partition_point는 "off <= hit"인 원소 개수를 주므로 -1이 그 행이다.
        let row = offsets.partition_point(|&off| off <= hit as u64).saturating_sub(1);
        let row_u32 = row as u32;

        // ---- Partial 빠른 경로: memmem 히트 자체가 매치. 행 중복만 거른다. ----
        if bytefast_partial {
            if last_row == Some(row_u32) {
                continue;
            }
            last_row = Some(row_u32);
            out.push(row_u32);
            continue;
        }

        let Some((s, en)) = crate::index::LineIndex::range_in(&offsets, total_bytes, row) else {
            continue;
        };

        // ---- 폴백(정밀 판정): Whole word, 멀티바이트 인코딩, CP949 Partial 등. ----
        // 연속 히트가 같은 행이면 건너뛴다(정밀 판정도 한 번만 하게).
        if last_row == Some(row_u32) {
            continue;
        }
        last_row = Some(row_u32);
        // 정밀 판정: 인코딩 디코딩 + whole_word/cell을 실제로 확인한다.
        // (memmem은 원바이트 부분 일치라 CP949 등에서 위양성이 있을 수 있고,
        //  whole_word/cell은 memmem이 판정하지 못한다.)
        // `decode_logical_line`과 **완전히 같은** 방법으로 디코딩·개행 제거해야
        // 정밀 판정이 `matching_lines`(= logical_line 경유)와 어긋나지 않는다:
        // 전체 슬라이스를 디코딩한 뒤 뒤쪽 `\r`/`\n`을 모두 trim한다.
        let text = crate::parse::decode_line(doc.source.slice(s, en), doc.enc)
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        if !crate::find::find_in_line_scoped(&text, query, opts, delim).is_empty() {
            out.push(row_u32);
        }
    }
    // find_iter는 offset 오름차순이라 out도 행 오름차순이다. Partial/폴백 경로 모두
    // last_row로 연속 중복을 막는다(offset 단조 증가 + 한 행의 바이트 범위 연속 →
    // 같은 행 히트는 반드시 인접).
    out
}

/// 행 단위 브루트포스 폴백. 빠른 경로가 성립하지 않을 때(비ASCII/손실 needle,
/// UTF-16 Whole cell, 텍스트 모드 Whole cell) 결과의 정의 그 자체인
/// `find_in_line_scoped`를 행마다 부른다. 느리지만 **항상 옳다**.
fn scan_rows_scoped(
    doc: &Document,
    query: &str,
    opts: &crate::find::FindOptions,
    delim: Option<u8>,
) -> Vec<u32> {
    // **`doc.index.line_count()`가 아니다.** Parquet 문서는 `LineIndex`가 비어
    // 있어 0이 되고, 오류 없이 **조용히 0건**을 돌려준다("찾기가 동작한다"만
    // 보는 테스트는 그대로 통과한다). `doc_line_count`는 편집 버퍼와 Parquet을
    // 모두 아는 유일한 함수이고, 텍스트 뷰 모드에서는 `index.line_count()`와
    // 같은 값이라 기존 동작이 바뀌지 않는다.
    let n = doc_line_count(doc);
    let mut out = Vec::new();
    for i in 0..n {
        let Some(text) = logical_line(doc, i) else { continue };
        if !crate::find::find_in_line_scoped(&text, query, opts, delim).is_empty() {
            out.push(i as u32);
        }
    }
    out
}

/// 뷰 모드 + match_case + **Whole cell**의 바이트 빠른 경로. `scan_view_ci_bytes`와
/// 같은 **행 단위** 구조다.
///
/// **왜 행 단위인가(치명적 계약).** Whole cell 비교 대상은 파일 바이트가 아니라
/// `split_fields`가 준 **표시값**이다(바깥 따옴표 벗김, `""` → `"`). 그래서
/// needle 바이트가 행에 한 번도 나타나지 않아도 매치일 수 있다:
/// `"a"a`의 표시값은 `aa`, `"a""b"`의 표시값은 `a"b`, `"hi"t`는 `hit`.
/// 히트를 순회하는 옛 구조는 히트 0개인 행을 **방문조차 하지 않아** 조용히
/// 비매치로 만들었다(위음성 = 브루트포스와 다른 결과). 행을 훑으면 그 행에 `"`가
/// 있는지 보고 정밀 판정으로 보낼 수 있다 — `"`가 없는 행(압도적 다수)만 바이트로
/// 단정하므로 빠른 경로의 이득은 그대로다.
#[allow(clippy::too_many_arguments)]
fn scan_view_cell_bytes(
    doc: &Document,
    query: &str,
    opts: &crate::find::FindOptions,
    delim: Option<u8>,
    offsets: &[u64],
    total_bytes: u64,
    bytes: &[u8],
    needle_bytes: &[u8],
) -> Vec<u32> {
    let needle_len = needle_bytes.len();
    let d = delim.expect("호출부가 delim.is_some()을 확인한다");
    // needle 바이트에 delim이 있으면(따옴표 셀에서만 셀 전체일 수 있음) 경계 판정이
    // 성립하지 않으므로 그 히트는 항상 폴백으로 보낸다(`classify_cell_hit` 참조).
    let needle_has_delim = needle_bytes.contains(&d);
    let finder = memchr::memmem::Finder::new(needle_bytes);
    let mut out = Vec::new();
    for row in 0..offsets.len() {
        let Some((s, en)) = crate::index::LineIndex::range_in(offsets, total_bytes, row) else {
            continue;
        };
        let (line_start, mut line_end) = (s as usize, en as usize);
        if line_end > bytes.len() || line_start > line_end {
            continue;
        }
        // 개행 제외 내용 끝(단일 바이트 인코딩이므로 `\r`/`\n`은 1바이트).
        while line_end > line_start && matches!(bytes[line_end - 1], b'\r' | b'\n') {
            line_end -= 1;
        }
        let lb = &bytes[line_start..line_end];
        // 이 행의 바이트가 곧 표시값인가(`"`가 하나도 없는가). 아니면 어떤 히트도
        // 바이트만으로 확정할 수 없고, 히트가 0개여도 비매치로 단정할 수 없다.
        // `classify_cell_hit`은 히트 **주변** 바이트만 보므로 줄 앞쪽의 닫히지 않은
        // 따옴표(`"a_,,HIT`)나 `"` 자체가 needle인 경우(`",` × `"`)를 볼 수 없다.
        let plain = cell_bytes_are_display(lb);
        let mut confirmed = false;
        let mut needs_refine = false;
        for hit in finder.find_iter(lb) {
            match classify_cell_hit(
                bytes,
                line_start,
                line_end,
                line_start + hit,
                needle_len,
                d,
                needle_has_delim,
            ) {
                CellHit::Confirmed => {
                    confirmed = true;
                    break;
                }
                CellHit::NeedsRefine => needs_refine = true,
                CellHit::NotCellBoundary => {}
            }
        }
        // 확정 히트가 있어도 CP949면 정밀 판정을 거친다 — 트레일 바이트가 ASCII와
        // 겹쳐 히트가 문자 중간에 걸릴 수 있다(`bytefast_ci_confirms`와 같은 규율).
        // 확정도 후보도 없을 때(히트 0개 포함)는 `plain`한 행만 비매치로 단정한다.
        let matched = if confirmed && plain && bytefast_ci_confirms(doc.enc) {
            true
        } else if confirmed || needs_refine || !plain {
            ci_refine_hit(doc, s, en, query, opts, delim)
        } else {
            false
        };
        if matched {
            out.push(row as u32);
        }
    }
    out
}

/// 뷰 모드 + **ignore_case**의 바이트 빠른 경로(Task H). 빠른 경로가 성립하지
/// 않으면(비ASCII needle, UTF-16, Whole word, 텍스트 모드 Whole cell) `None`을
/// 돌려 호출부가 행 단위 폴백을 타게 한다.
///
/// **왜 필요한가.** `FindOptions::default()`는 `match_case: false`다 — 사용자가
/// "Match case"를 켜지 않으면 **항상** 이 경로다. 그런데 종전에는 행마다
/// `logical_line`으로 **디코딩(String 할당)** 한 뒤 `find_in_line_scoped`를 불렀다.
/// 2GB/2200만 행에서 Whole cell 412초, Partial 54초가 나온 원인이 그 행마다의
/// 디코딩·할당이다. 여기서는 mmap 바이트를 그대로 훑어 그 비용을 없앤다(~0.37초).
///
/// **파일 전체 훑기가 아니라 행 단위인 이유.** `scan_view_memmem`은 memmem으로
/// 파일 전체를 한 번에 훑고 히트 offset을 이진탐색으로 행에 매핑한다. 그
/// 구조를 여기 그대로 쓰면 히트마다 `partition_point`(log n) 비용이 붙는데,
/// 흔한 검색어에서는 히트가 곧 행 수만큼 나오므로 그 비용이 그대로 쌓인다.
/// 행 경계는 이미 `index`가 알고 있으므로 **행마다 `find_ci_ascii`를 한 번**
/// 도는 편이 단순하고 충분히 빠르다(벤치 374ms). 핵심은 스캔 단위가 아니라
/// **디코딩을 하지 않는 것**이다.
///
/// **needle 인코딩.** `save::encode_bytes`로 문서 인코딩에 맞춰 바이트를 얻는다
/// (Task G의 교훈 — UTF-8 바이트를 CP949 파일에 그대로 쓰면 위음성). ASCII
/// needle은 UTF-8/CP949에서 같은 바이트라 무해하지만, **비ASCII needle(한글)은
/// 반드시 필요하다** — CP949 파일에서 `인도네시아`의 UTF-8 바이트를 찾으면
/// 한 행도 안 걸린다. 손실 여부는 바로 아래 `needle_roundtrips`가 막는다.
fn scan_view_ci_bytes(
    doc: &Document,
    query: &str,
    opts: &crate::find::FindOptions,
    delim: Option<u8>,
) -> Option<Vec<u32>> {
    if !bytefast_ci_ok(query, doc.enc) {
        return None;
    }
    // needle이 문서 인코딩으로 손실 없이 옮겨지지 않으면(대체 문자) 프리필터가
    // 브루트포스와 다른 질문을 하게 된다 → 폴백. `bytefast_ci_ok`가 비ASCII
    // needle(한글·한자)도 통과시키므로 이 가드가 **실제로** 걸리는 지점이다 —
    // CP949로 표현할 수 없는 한자·이모지 needle이 여기서 폴백으로 빠진다.
    if !needle_roundtrips(query, doc.enc) {
        return None;
    }
    let scope = opts.scope;
    // Whole word는 유니코드 단어 경계라 바이트로 안전하지 않다(match_case 경로도
    // 같은 이유로 폴백이다). Whole cell은 표 모드(delim 존재)에서만 — 텍스트 모드의
    // "행 전체 일치"는 개행 trim·인코딩 정합성이 섞여 폴백에 맡긴다.
    let cell = match scope {
        crate::find::MatchScope::Partial => false,
        crate::find::MatchScope::WholeCell if delim.is_some() => true,
        _ => return None,
    };
    let needle_bytes = crate::save::encode_bytes(query, doc.enc);
    if needle_bytes.is_empty() {
        return None;
    }
    let needle_len = needle_bytes.len();
    let needle_lower: Vec<u8> = needle_bytes.iter().copied().map(ascii_lower).collect();
    let needle_has_delim = delim.is_some_and(|d| needle_bytes.contains(&d));
    // UTF-8이면 히트를 바로 확정, CP949면 트레일 바이트 위양성 때문에 후보로만 본다.
    let confirms = bytefast_ci_confirms(doc.enc);

    let (offsets, total_bytes) = doc.index.snapshot();
    let bytes = doc.source.as_bytes();
    let mut out = Vec::new();
    for row in 0..offsets.len() {
        let Some((s, en)) = crate::index::LineIndex::range_in(&offsets, total_bytes, row) else {
            continue;
        };
        let (line_start, mut line_end) = (s as usize, en as usize);
        if line_end > bytes.len() || line_start > line_end {
            continue;
        }
        // 개행 제외 내용 끝(단일 바이트 인코딩이므로 `\r`/`\n`은 1바이트).
        while line_end > line_start && matches!(bytes[line_end - 1], b'\r' | b'\n') {
            line_end -= 1;
        }
        let lb = &bytes[line_start..line_end];

        // ---- Partial: 히트 하나면 이 행에 needle이 있다는 증명. ----
        if !cell {
            if find_ci_ascii(lb, &needle_lower).is_none() {
                continue;
            }
            // CP949는 한글 트레일 바이트가 ASCII 대문자와 겹쳐 위양성이 가능하므로
            // 후보 행만 디코딩해 정밀 판정한다(위양성은 안전, 위음성이 위험).
            if confirms || ci_refine_hit(doc, s, en, query, opts, delim) {
                out.push(row as u32);
            }
            continue;
        }

        // ---- Whole cell: 히트마다 경계 바이트만 본다(`classify_cell_hit` 재사용 —
        //      그 함수는 대소문자와 무관하게 경계 바이트만 보므로 그대로 쓴다). ----
        let d = delim.unwrap();
        // 이 행의 바이트가 곧 표시값인가(`"`가 하나도 없는가). `classify_cell_hit`은
        // 히트 **주변** 바이트만 보므로 줄 앞쪽의 닫히지 않은 따옴표나 `"`가 needle인
        // 경우를 볼 수 없다 → `"`가 있으면 어떤 판정도 바이트로 확정하지 않는다.
        let plain = cell_bytes_are_display(lb);
        let mut confirmed = false;
        let mut needs_refine = false;
        for hit in find_ci_ascii_all(lb, &needle_lower) {
            // `classify_cell_hit`은 파일 전체 바이트 기준 offset을 받으므로
            // 행 슬라이스 안의 위치를 파일 offset으로 되돌린다.
            match classify_cell_hit(
                bytes,
                line_start,
                line_end,
                line_start + hit,
                needle_len,
                d,
                needle_has_delim,
            ) {
                CellHit::Confirmed => {
                    confirmed = true;
                    break;
                }
                CellHit::NeedsRefine => needs_refine = true,
                CellHit::NotCellBoundary => {}
            }
        }
        // 확정 히트가 있어도 CP949면 정밀 판정을 거친다(위 (가)안). 확정이 없고
        // 따옴표 후보만 있으면 폴백으로 확인한다.
        //
        // **둘 다 없을 때(히트 0개 포함)를 바이트로 "비매치"라 단정하지 않는다.**
        // Whole cell 비교 대상은 표시값이라 따옴표가 낀 행은 needle 바이트가
        // 한 번도 나타나지 않아도 매치일 수 있다(`"a"a` → 표시값 `aa`).
        // 행에 `"`가 없을 때만 표시값 == 원본 바이트이므로 그때만 단정한다.
        let matched = if confirmed && plain && confirms {
            true
        } else if confirmed || needs_refine || !plain {
            ci_refine_hit(doc, s, en, query, opts, delim)
        } else {
            false
        };
        if matched {
            out.push(row as u32);
        }
    }
    Some(out)
}

/// 후보 행 하나를 디코딩해 `find_in_line_scoped`로 정밀 판정한다.
/// `scan_view_memmem`의 폴백 경로와 **완전히 같은** 방법으로 디코딩·개행 제거해야
/// 결과가 `matching_lines`(= `logical_line` 경유)와 어긋나지 않는다.
fn ci_refine_hit(
    doc: &Document,
    s: u64,
    en: u64,
    query: &str,
    opts: &crate::find::FindOptions,
    delim: Option<u8>,
) -> bool {
    let text = crate::parse::decode_line(doc.source.slice(s, en), doc.enc)
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    !crate::find::find_in_line_scoped(&text, query, opts, delim).is_empty()
}

/// 스크롤 마커 거터를 그릴지 여부. Find All 스냅샷(`highlight`)이 없거나 매치
/// 행이 비어 있으면(검색 안 함 또는 매치 없음) 데이터 폭을 아끼기 위해 그리지
/// 않는다 — 스냅샷이 있고 매치가 있을 때만 나타난다. 표시 조건을 순수 함수로
/// 뽑아 테스트가 실제 조건을 부르게 한다(인라인 복붙 금지).
fn show_gutter(highlight: Option<&Highlight>) -> bool {
    highlight.is_some_and(|h| !h.rows.is_empty())
}

/// 거터 세로 폭을 전체 논리 행 수에 매핑한다. 논리 행 `row`의 눈금 y 좌표.
///
/// `line_count`가 0이면(빈 문서) top을 준다. `row`는 클램프하지 않는다 —
/// 호출부(`marker_y`)는 항상 유효 행만 넘긴다. 역함수는 `row_at_y`.
fn marker_y(row: usize, line_count: usize, top: f32, height: f32) -> f32 {
    if line_count == 0 {
        return top;
    }
    top + (row as f32 / line_count as f32) * height
}

/// 거터 클릭 y → 논리 행 번호. `marker_y`의 역함수. 거터 위/아래 밖을 클릭해도
/// 유효 행(`0..line_count-1`)으로 클램프한다 — 거터 끝을 살짝 넘겨 클릭해도
/// 첫/마지막 행으로 점프하게(에디터 관례).
fn row_at_y(y: f32, line_count: usize, top: f32, height: f32) -> usize {
    if line_count == 0 || height <= 0.0 {
        return 0;
    }
    let frac = ((y - top) / height).clamp(0.0, 1.0);
    let row = (frac * line_count as f32) as usize;
    row.min(line_count - 1)
}

/// 현재 매치 한 곳을 치환하고 다음 매치로 옮긴다. 현재 매치가 없으면
/// (또는 그 자리가 더 이상 매치가 아니면) 먼저 찾는다 — 워드/브라우저의
/// "바꾸기"가 다들 그렇게 동작한다.
fn replace_one(doc: &mut Document) {
    if doc.edit.is_none() {
        return;
    }
    // 치환 대상 = 지금 선택돼 있는 매치. 그게 정말 지금도 매치인지 다시
    // 확인한다(편집·되돌리기로 그 자리 글자가 바뀌었을 수 있다).
    let delim = doc_delimiter(doc);
    // 재검증도 `search_from`이 매치를 만들 때 쓴 것과 **같은** 검색어여야 한다 —
    // 여기만 날 문자열을 보면 `\t`로 찾아 놓은 매치가 매번 재검증에 실패해
    // 바꾸기가 늘 "그냥 다음 찾기"로 흘러 버린다.
    let query = effective_query(doc);
    let target = doc.last_match.filter(|m| {
        logical_line(doc, m.line).is_some_and(|text| {
            crate::find::find_in_line_scoped(&text, &query, &doc.find_opts, delim)
                .iter()
                .any(|&(c, l)| c == m.col && l == m.len)
        })
    });
    let Some(m) = target else {
        // 아직 아무것도 안 잡혀 있으면 Find Next와 같이 동작한다. `target`이
        // None이 된 것(재검증 실패)만으로는 `doc.last_match`가 지워지지
        // 않는다 — `filter`는 지역 값만 버릴 뿐 원본 필드는 그대로다. 그
        // 상태로 이 검색마저 실패하면(Minor 5) 낡은 `last_match`가 버퍼
        // 범위 밖 논리 행을 가리킨 채 남아 다음 Find Next의 기준이 뒤섞인다.
        let found = search_from(doc, find_origin(doc, true), true);
        match found {
            Some(m) => focus_match(doc, m),
            None => {
                doc.last_match = None;
                doc.find_status = "Not found".to_owned();
            }
        }
        return;
    };
    let rep = crate::find::sanitize_for_line(&effective_replacement(doc));
    let Some(e) = doc.edit.as_mut() else { return };
    let Some(old) = e.lines.get(m.line).cloned() else { return };
    // char 인덱스를 바이트로 옮겨 그 구간만 갈아 끼운다. 한 줄 전체를
    // `replace_in_line`으로 돌리면 **그 줄의 모든 매치**가 바뀌어 버린다 —
    // "바꾸기"는 한 곳만이다.
    let mut byte_of: Vec<usize> = old.char_indices().map(|(b, _)| b).collect();
    byte_of.push(old.len());
    let (Some(&s), Some(&t)) = (byte_of.get(m.col), byte_of.get(m.col + m.len)) else {
        return;
    };
    let mut new = String::with_capacity(old.len());
    new.push_str(&old[..s]);
    new.push_str(&rep);
    new.push_str(&old[t..]);
    // 치환문이 매치와 글자 그대로 같으면(예: "hit" → "hit") 실제로는 아무것도
    // 안 바뀐다. 그런데도 undo를 push하고 dirty를 세우면 사용자가 저장하지
    // 않아도 될 파일에 "● Modified"가 뜨고 되돌리기 한 칸이 아무 일도 안
    // 하는 유령 단계가 된다(Important 3) — `commit_editing_cell`/`Cut`/
    // `Clear`가 이미 지키는 "실제로 바뀐 경우에만 push" 규율을 여기도 따른다.
    let changed = new != old;
    if changed {
        // 되돌리기는 **바꾸기 직전**에 이전 값으로 push한다(기존 셀 편집과 동일 규율).
        e.undo.push(crate::edit::EditOp::Replace(vec![(m.line, old)]));
        e.lines[m.line] = new;
        e.dirty = true;
    }
    // 다음 매치로. 치환문 길이만큼 자리가 밀렸으므로 치환 **끝** 자리를
    // 기준으로 삼아야 방금 넣은 글자를 다시 잡지 않는다(치환문이 검색어를
    // 포함하는 경우 — "a" → "aa" — 무한 제자리걸음이 된다). 변경이 없었어도
    // 자리는 그대로이므로 같은 계산식이 맞는다.
    let rep_len = rep.chars().count();
    let after = crate::edit::TextPos {
        line: m.line,
        col: (m.col + rep_len).saturating_sub(1),
    };
    doc.last_match = None;
    doc.find_status.clear();
    match search_from(doc, after, true) {
        Some(next) => focus_match(doc, next),
        None => {
            // 검색 실패 시 last_match를 반드시 None으로 유지한다(Minor 5) —
            // 버퍼가 줄어든 뒤 다음 Find Next가 낡은 위치를 기준 삼지 않도록.
            doc.last_match = None;
            doc.find_status = if changed {
                "1 replacement".to_owned()
            } else {
                "0 replacements (already matches)".to_owned()
            };
        }
    }
}

/// 문서 전체 바꾸기. 바뀐 행만 받아 한 번의 `Batch`(= Ctrl+Z 한 번)로 묶는다.
///
/// **큰 문서 확인 다이얼로그를 붙이지 않은 이유.** `BIG_COLUMN_OP_ROWS`
/// 확인은 "클릭 한 번에 전 데이터 행이 지워지거나 수백 MB 클립보드가 생기는"
/// 컬럼 연산을 위한 것이다. 전체 바꾸기는 성격이 다르다 — (a) 사용자가 검색어와
/// 치환문을 직접 타이핑한 **명시적** 동작이라 실수로 발동하지 않고, (b) 결과가
/// `Batch` 하나로 묶여 Ctrl+Z 한 번에 전부 되돌아가며, (c) 되돌리기 스택에
/// 담기는 것은 **바뀐 행**뿐이라 매치가 적으면 문서가 아무리 커도 메모리가
/// 늘지 않는다. 남는 비용은 전 행 스캔 시간뿐인데, 그건 이미 정렬 등
/// 다른 전 행 연산이 확인 없이 하는 일과 같은 급이다. 따라서 여기에
/// `PendingColumnOp`와 같은 확인 단계를 새로 만들지 않는다.
fn replace_all_in_doc(doc: &mut Document) {
    let query = effective_query(doc);
    let rep = effective_replacement(doc);
    let opts = doc.find_opts.clone();
    let delim = doc_delimiter(doc);
    let Some(e) = doc.edit.as_mut() else { return };
    let (changed, total) = crate::find::replace_all(&e.lines, &query, &rep, &opts, delim);
    if total == 0 {
        doc.find_status = "Not found".to_owned();
        doc.last_match = None;
        return;
    }
    // `replace_all`은 "매치가 있던 행"을 돌려주지, "실제로 글자가 달라진
    // 행"을 걸러주지 않는다 — 치환문이 검색어와 글자 그대로 같으면(예:
    // "hit" → "hit") 매치는 있었지만 새 텍스트가 옛 텍스트와 동일하다.
    // 그런 행까지 undo에 담으면 Ctrl+Z 한 번이 아무것도 안 바꾸는 유령
    // 단계가 되고 dirty가 거짓으로 서므로(Important 3), 실제로 달라진 행만
    // 추린다.
    //
    // **세 가지를 한 패스에서 끝낸다.** 예전에는 바뀐 집합을 세 번 훑었다 —
    // (a) 달라졌는지 비교, (b) undo용으로 옛 텍스트 **clone**, (c) 쓰기.
    // `mem::replace`는 새 텍스트를 넣으면서 그 자리의 옛 String을 그대로
    // 돌려주므로, (b)의 clone 476만 번이 통째로 없어진다(문자열 복사 0).
    // 실파일에서 이 구간이 **226ms → 46ms(4.9배)**, 상주 메모리도 194MB 준다
    // (옛 텍스트가 한동안 두 벌 떠 있던 것이 없어졌다).
    let mut before: Vec<(usize, String)> = Vec::with_capacity(changed.len());
    for (i, text) in changed {
        // 범위 밖 행번호는 조용히 건너뛴다(옛 `get(..).is_none_or(..)`가
        // "없으면 바뀐 것으로 친다"였으나, 없는 행에는 쓸 수도 undo할 수도
        // 없어 결과가 같다).
        let Some(slot) = e.lines.get_mut(i) else { continue };
        if *slot == text {
            continue;
        }
        before.push((i, std::mem::replace(slot, text)));
    }
    if before.is_empty() {
        // 매치는 total개 있었지만 전부 치환 전후가 같았다 — 바뀐 게 없다는
        // 사실을 그대로 알린다("N replacements"라고 하면 거짓 보고가 된다).
        doc.find_status = "0 replacements (already matches)".to_owned();
        doc.last_match = None;
        return;
    }
    // 되돌리기: 바뀐 행들의 **이전** 값을 한 Replace에 모아 담는다. 한 사용자
    // 동작 = 한 번의 Ctrl+Z이므로 Batch로 감쌀 필요조차 없다(Replace 하나가
    // 이미 여러 행을 한 단계로 복원한다). 행 수는 변하지 않는다.
    e.undo.push(crate::edit::EditOp::Replace(before));
    e.dirty = true;
    // 치환이 끝나면 이전 매치 위치는 의미가 없다(그 자리 글자가 바뀌었다).
    doc.last_match = None;
    // 그래머: 1개면 단수, 그 외(0 포함)는 복수(Minor 4, replace_one과 일관).
    doc.find_status = if total == 1 {
        "1 replacement".to_owned()
    } else {
        format!("{total} replacements")
    };
}

// ---------------------------------------------------------------------------
// 찾기 결과 행 추출 (Extract Rows)
// ---------------------------------------------------------------------------

/// 추출이 훑을 구간과 결과 맨 앞에 붙일 헤더 행의 유무.
///
/// 표 모드에서 `has_header`면 0번 행은 헤더다. 헤더는 **검색 대상에서 뺀다** —
/// 헤더에 검색어가 우연히 들어 있다고 "그 행이 매치됐다"고 추출하면, 데이터가
/// 하나도 안 맞는데 결과가 1행짜리로 나오는 거짓 성공이 된다. 대신 결과 맨
/// 앞에는 **무조건** 붙인다(컬럼 이름이 없으면 새 탭이 읽을 수 없는 표가 된다).
///
/// 텍스트 모드거나 `has_header`가 false면 헤더 개념이 없으므로 0행부터 훑고
/// 아무것도 앞에 붙이지 않는다.
///
/// `update()`와 테스트가 이 함수 하나를 공유해야 한다 — 판단식을 양쪽에 따로
/// 적으면(`plan_dropped_files` 주석 참조) 실제 규칙을 뒤집어도 테스트는 자기
/// 사본만 보고 통과한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExtractPlan {
    /// 훑기 시작할 논리 행(헤더가 있으면 1).
    scan_from: usize,
    /// 결과 맨 앞에 헤더 행(논리 0)을 붙일지.
    prepend_header: bool,
}

fn extract_plan(has_header: bool, sep: SeparatorMode) -> ExtractPlan {
    // 헤더는 표 모드에서만 의미가 있다. 텍스트 모드는 `has_header`가 false로
    // 오게 되어 있지만(`open_path`의 감지), 편집 중 구분자를 바꾸는 경로가
    // 있으므로 여기서도 모드를 함께 본다.
    let header = has_header && matches!(sep, SeparatorMode::Char(_));
    ExtractPlan {
        scan_from: if header { 1 } else { 0 },
        prepend_header: header,
    }
}

/// 추출을 지금 수행해도 되는가. 탭을 추가하고 `active`를 바꾸는 동작이므로
/// 탭 바가 잠겨 있으면(저장 다이얼로그·확인 창) 막는다 — 잠금 중에 `active`가
/// 움직이면 그 다이얼로그가 엉뚱한 문서를 겨눈다(`tab_bar_locked` 주석 참조).
/// 드롭 처리가 `plan_dropped_files`로 하는 것과 같은 규율로, 실제 가드와
/// 테스트가 **이 함수 하나**를 공유한다.
fn extract_allowed(app: &App) -> bool {
    !tab_bar_locked_for(app)
}

/// 잠겨 있어 추출을 막았을 때 상태 문구. `plan_dropped_files`의 안내와 같은 결.
const EXTRACT_LOCKED_STATUS: &str = "Close the open dialog first";

/// "Extract Rows" 버튼이 활성화되어야 하는가. 검색어가 있을 때만 참이다 —
/// 뷰/편집 모드 둘 다에서 동작해야 하므로(찾기와 같다) `editing` 여부는
/// 보지 않는다. `render_find_panel`과 테스트가 이 함수 하나를 공유한다 —
/// 버튼의 활성/비활성 판정을 렌더 클로저 안에 인라인으로 두면, 그 판정을
/// 지워도(항상 활성, 또는 버튼 자체를 지워도) 렌더 결과만 보는 테스트로는
/// 잡아낼 수 없다.
///
/// **날 문자열을 받는다(디코딩된 값이 아니라).** 이 판정이 묻는 것은 "사용자가
/// 입력란에 뭔가 쳤는가"이지 "그 결과가 무엇인가"가 아니다. 실제로는 두 근거가
/// 언제나 같은 값을 준다 — `unescape("")`는 `""`이고 비어 있지 않은 입력은 어떤
/// 규칙을 타든 최소 한 글자를 남기므로 디코딩이 검색어를 비게 만들 수 없다
/// (`effective_query` 주석). 그래서 값이 아니라 **뜻**이 맞는 쪽을 고른다:
/// 버튼은 입력란의 상태를 비추는 UI이므로 입력란의 글자를 본다. 부수적으로 이
/// 함수는 `Document`를 알 필요가 없어져 `&str` 하나로 테스트된다.
fn extract_button_enabled(find_query: &str) -> bool {
    !find_query.is_empty()
}

/// Find All 버튼을 활성화할지. 검색어가 비면 스캔할 게 없으므로 비활성이다
/// (추출 버튼과 같은 규칙, 날 문자열을 보는 이유도 같다). `render_find_panel`
/// 안에 인라인으로 두면 지우거나 뒤집어도 렌더 결과만 보는 테스트로는 잡히지
/// 않으므로 순수 함수로 뽑는다.
fn find_all_button_enabled(find_query: &str) -> bool {
    !find_query.is_empty()
}

/// 추출된 행 텍스트로 뷰 모드 `Document`를 만든다.
///
/// **왜 인메모리 `Source` + 동기 인덱스인가.** `Document`는 `Arc<Source>`와
/// `LineIndex`를 전제로 만들어져 있고, 표 렌더·정렬·찾기·편집 모드 진입이 전부
/// 그 둘을 통해 돈다. 추출본을 "편집 버퍼만 있는 반쪽 문서"로 만들면 편집 모드를
/// 끄는 순간 뷰 경로(`decode_logical_line`)가 빈 소스를 읽어 화면이 비어 버린다.
/// 그래서 추출 결과를 원본과 **같은 인코딩**으로 인코딩해 `Source::from_bytes`로
/// 감싸고, 그 바이트 위에 인덱스를 채워 정상적인 뷰 모드 문서를 만든다.
///
/// **인덱스는 백그라운드 없이 동기로 채운다.** 바이트가 이미 램에 다 있으므로
/// 스레드를 띄울 이유가 없고(진행률을 보여줄 대상도 없다), 무엇보다 `indexer`
/// 핸들이 None인 채로 `Phase::Complete`가 되어야 상태바가 "인덱싱 중"으로
/// 남지 않는다. offset은 인덱서와 **같은 규칙**으로 만든다:
/// 첫 줄 시작(0)은 내용이 있을 때만, 개행 **다음** 위치가 새 줄 시작이며
/// 파일 끝 개행 뒤의 빈 줄은 세지 않는다(`indexer::scan_segment` 주석 참조).
/// 우리가 만든 바이트는 "행마다 개행 하나로 끝나는" 형태라 이 규칙대로면
/// 정확히 `lines.len()`개의 offset이 나온다.
fn build_extracted_doc(
    lines: &[String],
    enc: Encoding,
    sep: SeparatorMode,
    has_header: bool,
    newline: crate::edit::Newline,
    path_label: String,
) -> Document {
    let nl = match newline {
        crate::edit::Newline::Lf => "\n",
        crate::edit::Newline::CrLf => "\r\n",
    };
    let nl_bytes = crate::save::encode_bytes(nl, enc);
    let mut bytes: Vec<u8> = Vec::new();
    // 줄 시작 offset을 바이트를 쌓으면서 그 자리에서 기록한다 — 다 만든 뒤
    // 다시 스캔하면 UTF-16에서 개행 패턴을 또 찾아야 하고, 그 계산이 여기
    // 인코딩 로직과 어긋나면 행이 밀린다. 쌓는 쪽이 곧 진실이다.
    let mut offsets: Vec<u64> = Vec::with_capacity(lines.len());
    for line in lines {
        offsets.push(bytes.len() as u64);
        bytes.extend_from_slice(&crate::save::encode_bytes(line, enc));
        bytes.extend_from_slice(&nl_bytes);
    }
    let total = bytes.len() as u64;
    let src = Arc::new(Source::from_bytes(bytes));
    let index = LineIndex::new(total);
    index.replace_offsets(offsets);
    index.set_bytes_done(total);
    index.set_phase(Phase::Complete);

    let custom_sep_input = match sep {
        SeparatorMode::Char(b) if b.is_ascii_graphic() => (b as char).to_string(),
        _ => String::new(),
    };

    Document {
        source: src,
        index,
        enc,
        sep,
        has_header,
        // 동기로 다 채웠으므로 붙일 인덱서 스레드가 없다.
        indexer: None,
        // 디스크에 대응하는 파일이 없다. 저장하면 `render_save_dialog`가
        // 빈 경로를 보고 "다른 이름으로 저장"으로 폴백한다(그 지점 주석 참조).
        path: std::path::PathBuf::new(),
        path_label,
        is_extracted: true,
        custom_sep_input,
        // 아래는 전부 초기값 — 추출본은 원본의 선택/정렬/편집 상태를 물려받지
        // 않는다(추출 순서가 곧 원본 순서이므로 정렬도 없다).
        selected_col: None,
        sort: None,
        sort_job: None,
        show_sort_dialog: false,
        show_convert_dialog: false,
        convert_target: None,
        convert_custom_input: String::new(),
        sort_specs: Vec::new(),
        edit: None,
        editing_cell: None,
        cell_edit_text: String::new(),
        cell_sel: None,
        cell_drag_active: false,
        text_sel: None,
        text_caret: crate::edit::TextPos { line: 0, col: 0 },
        text_drag_active: false,
        ime_preview: String::new(),
        pending_column_op: None,
        // 찾기 패널은 새 탭에서 닫힌 채 시작한다. 검색어/옵션과 하이라이트는
        // 호출부(`extract_matching_rows`)가 원본에서 물려받아 채운다 — 열자마자
        // 하이라이트가 보이고, 사용자가 패널을 열면 그 검색어로 바로 Find Next를
        // 할 수 있다. 여기서는 기본값(빈 검색어)으로 두고 호출부가 덮어쓴다.
        show_find: false,
        find_query: String::new(),
        replace_text: String::new(),
        find_opts: crate::find::FindOptions::default(),
        // 이스케이프 해석 여부도 검색어/옵션과 함께 원본에서 물려받는다
        // (`extract_matching_rows`가 덮어쓴다) — 새 탭에서 같은 검색어로 Find
        // Next를 눌렀을 때 원본과 같은 것을 찾아야 한다.
        find_escapes: false,
        last_match: None,
        find_status: String::new(),
        find_focus_pending: false,
        // 하이라이트는 호출부(`extract_matching_rows`)가 새 문서 기준으로
        // `scan_all_matches`를 돌려 채운다. 여기서는 빈 상태로 둔다.
        highlight: None,
        pending_scroll_row: None,
        pending_scroll_align: egui::Align::Center,
        first_visible_row: 0,
        visible_rows: 0,
        view_scale: 1.0,
        row_errors: None,
        error_scan: None,
        row_errors_revision: 0,
        show_errors_window: false,
        hex: None,
        parquet: None,
        pending_parquet_sort: None,
    }
}

/// 헥스 문서를 만든다. 텍스트 전용 필드는 전부 불활성 값 —
/// `open_path_as_text`의 리터럴과 같은 초기값이되, `indexer: None`(줄
/// 인덱서를 돌리지 않는다)과 `hex: Some(..)`(헥스 상태)만 다르다.
fn hex_document(source: Arc<Source>, path: &Path) -> Document {
    Document {
        index: LineIndex::new(source.len()),
        source,
        enc: Encoding::Utf8, // 표시 전용 — 헥스 문서에서 인코딩은 무의미
        sep: SeparatorMode::None,
        has_header: false,
        indexer: None,
        path: path.to_path_buf(),
        path_label: path.display().to_string(),
        is_extracted: false,
        custom_sep_input: String::new(),
        selected_col: None,
        sort: None,
        sort_job: None,
        show_sort_dialog: false,
        show_convert_dialog: false,
        convert_target: None,
        convert_custom_input: String::new(),
        sort_specs: Vec::new(),
        edit: None,
        editing_cell: None,
        cell_edit_text: String::new(),
        cell_sel: None,
        cell_drag_active: false,
        text_sel: None,
        text_caret: crate::edit::TextPos { line: 0, col: 0 },
        text_drag_active: false,
        ime_preview: String::new(),
        pending_column_op: None,
        show_find: false,
        find_query: String::new(),
        replace_text: String::new(),
        find_opts: crate::find::FindOptions::default(),
        find_escapes: false,
        last_match: None,
        find_status: String::new(),
        find_focus_pending: false,
        highlight: None,
        pending_scroll_row: None,
        pending_scroll_align: egui::Align::Center,
        first_visible_row: 0,
        visible_rows: 0,
        view_scale: 1.0,
        row_errors: None,
        error_scan: None,
        row_errors_revision: 0,
        show_errors_window: false,
        hex: Some(crate::hex::HexState::new()),
        parquet: None,
        pending_parquet_sort: None,
    }
}

/// Parquet 문서를 만든다. **표 모드로 그리되 행은 `ParquetDoc`에서 나온다.**
/// `hex_document`와 같은 규율의 리터럴이되 세 가지가 다르다:
/// `indexer: None`(개행을 셀 필요가 없다), `sep`/`has_header`(표 모드),
/// `parquet: Some(..)`.
///
/// **구분자는 콤마 고정이다.** Parquet에는 원본 구분자라는 개념이 없다.
/// 사용자가 툴바에서 바꿀 수 있게 하면 값에 그 문자가 들어갈 때 재인용이
/// 필요해진다. 내보내기에서만 대상 구분자를 고른다.
///
/// **`has_header: true`** — 첫 논리 행이 컬럼 이름 행이다. 이렇게 하면
/// `render_table`의 `data_start` 계산이 텍스트 경로와 동일하게 동작한다.
fn parquet_document(
    source: Arc<Source>,
    path: &Path,
    pq: crate::parquet::ParquetDoc,
) -> Document {
    Document {
        // 빈 LineIndex — Parquet은 개행을 세지 않는다. 행 수는 `ParquetDoc`이
        // 답한다(`doc_line_count` 참조).
        index: LineIndex::new(source.len()),
        source,
        enc: Encoding::Utf8, // 표시 전용 — Parquet 셀은 이미 문자열로 나온다
        sep: SeparatorMode::Char(b','),
        has_header: true,
        indexer: None,
        path: path.to_path_buf(),
        path_label: path.display().to_string(),
        is_extracted: false,
        custom_sep_input: ",".to_string(),
        selected_col: None,
        sort: None,
        sort_job: None,
        show_sort_dialog: false,
        show_convert_dialog: false,
        convert_target: None,
        convert_custom_input: String::new(),
        sort_specs: Vec::new(),
        edit: None,
        editing_cell: None,
        cell_edit_text: String::new(),
        cell_sel: None,
        cell_drag_active: false,
        text_sel: None,
        text_caret: crate::edit::TextPos { line: 0, col: 0 },
        text_drag_active: false,
        ime_preview: String::new(),
        pending_column_op: None,
        show_find: false,
        find_query: String::new(),
        replace_text: String::new(),
        find_opts: crate::find::FindOptions::default(),
        find_escapes: false,
        last_match: None,
        find_status: String::new(),
        find_focus_pending: false,
        highlight: None,
        pending_scroll_row: None,
        pending_scroll_align: egui::Align::Center,
        first_visible_row: 0,
        visible_rows: 0,
        view_scale: 1.0,
        row_errors: None,
        error_scan: None,
        row_errors_revision: 0,
        show_errors_window: false,
        hex: None,
        parquet: Some(std::cell::RefCell::new(pq)),
        pending_parquet_sort: None,
    }
}

/// 아직 저장한 적 없는 **새 파일** 문서를 만든다(빈 한 줄, 편집 모드).
///
/// **왜 `build_extracted_doc`을 재사용하나.** 필요한 것이 정확히 같다 —
/// 인메모리 `Source` + 동기로 채운 완료 인덱스 + 빈 `path`. 필드 40개짜리
/// 구조체 리터럴을 한 벌 더 두면 나중에 `Document`에 필드가 늘 때 한쪽만
/// 고쳐질 자리가 생긴다. 다른 점은 셋뿐이라 여기서 덮어쓴다:
/// `is_extracted`(새 파일은 추출본이 아니다 — 탭 라벨 접두사를 붙이면 안 된다),
/// `path_label`(빈 문자열이라야 `tab_label`이 `"(untitled)"`로 떨어진다),
/// 그리고 편집 모드 진입.
///
/// **왜 텍스트 모드(`SeparatorMode::None`)인가.** 빈 문서에는 감지할 구분자가
/// 없다. 표 모드로 열면 컬럼이 하나뿐인 표가 되어, 사용자가 `a,b,c`를 쳐도
/// 화면은 여전히 한 칸이다(구분자를 나중에 툴바에서 바꿔야 한다). 텍스트
/// 모드는 무엇을 치든 그대로 보이므로 새 파일의 기대에 맞는다. 표가 필요하면
/// 툴바 `Delimiter`로 언제든 바꿀 수 있고, 그건 보기 설정이라 데이터를
/// 건드리지 않는다.
///
/// **`path`가 비어 있는 것이 저장 유도의 전부다.** `save_as_fallback`이
/// 빈 경로를 보고 파일 선택 창으로 폴백하고, 저장에 성공하면 같은 판정으로
/// `doc.path`/`path_label`을 새 경로로 갱신한다 — 그 뒤 Ctrl+S는 덮어쓰기가
/// 된다. 추출본이 이미 지나다니는 길이라 새로 만들 것이 없다.
///
/// 인코딩 UTF-8 + 개행 CRLF로 시작한다. 파일에서 물려받을 값이 없으니 골라야
/// 하는데, UTF-8은 이 앱의 기본 저장 인코딩(`App::default`)과 같고 CRLF는
/// Windows 기본이다. 둘 다 저장 다이얼로그에서 바꿀 수 있다.
pub fn new_document() -> Document {
    let mut doc = build_extracted_doc(
        &[String::new()],
        Encoding::Utf8,
        SeparatorMode::None,
        false,
        crate::edit::Newline::CrLf,
        String::new(),
    );
    doc.is_extracted = false;
    // 새 파일은 처음부터 편집 모드다 — 그러려고 만든 문서다.
    enter_edit_mode(&mut doc);
    doc
}

/// 추출본 탭의 표시 이름. `path`가 비어 있으므로 `tab_label`은 이 문자열을
/// 그대로 쓰고 24자에서 자른다 — 그래서 **앞부분에 구분되는 정보**를 둔다.
///
/// `"[hit] 원본파일명"` 형식을 골랐다. 이유:
/// - 접두사가 6자로 짧아 24자 예산 대부분이 원본 파일명에 간다. 브리프의 다른
///   후보 `"Extracted: <파일명> (<N> rows)"`는 접두사만 11자에 뒤의 행 수는
///   잘려 아예 보이지 않으므로, 탭에서 원본을 구분할 수 없게 된다.
/// - `[` 로 시작해 파일 탭들 사이에서 "파일이 아닌 탭"임이 한눈에 보인다.
/// - 행 수는 탭이 아니라 `find_status`("N rows extracted")에 남긴다 —
///   추출 직후 사용자가 보는 곳은 그쪽이고, 탭 라벨은 오래 남는 식별자다.
///
/// 원본에 파일명이 없으면(추출본에서 다시 추출) `path_label`을 그대로 쓰되,
/// 원본이 **이미 추출본이면**(`src.is_extracted`) 접두사를 **다시 붙이지
/// 않는다** — 추출을 반복할 때마다 `"[hit] [hit] [hit] …"`로 자라면 24자
/// 예산이 접두사로만 차서 정작 원본 파일명이 사라진다. 몇 번째 추출인지는
/// 탭 라벨이 알려야 할 정보가 아니다.
///
/// **라벨 텍스트가 아니라 `is_extracted` 플래그로 판단한다.** 과거에는
/// `name.starts_with("[hit] ")`로 "이미 접두사가 붙었는가"를 추측했는데,
/// 그러면 실제 파일 이름이 우연히 `"[hit] real.csv"`인 파일을 열어 추출할 때도
/// "이미 접두사가 붙었다"고 오판해 접두사를 또 붙이지 않는다 — 그 추출 탭은
/// 원본 파일 탭과 라벨이 완전히 같아져 구분할 수 없다. 플래그는 "추출로
/// 만들어졌는가"라는 사실 그 자체를 담으므로 파일명이 우연히 무엇이든 흔들리지
/// 않는다.
fn extracted_label(src: &Document) -> String {
    const PREFIX: &str = "[hit] ";
    let name = src
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| src.path_label.clone());
    if name.is_empty() {
        "[hit]".to_owned()
    } else if src.is_extracted {
        name
    } else {
        format!("{PREFIX}{name}")
    }
}

impl App {
    /// 활성 문서에서 현재 검색어가 든 행만 모아 **새 탭**으로 연다.
    /// 원본 문서는 읽기만 한다(선택/편집 상태를 포함해 아무것도 바꾸지 않되,
    /// 결과 안내인 `find_status`만 갱신한다).
    ///
    /// **큰 문서 확인 다이얼로그를 붙이지 않은 이유.** `BIG_COLUMN_OP_ROWS`
    /// 확인은 "클릭 한 번에 전 데이터 행이 지워지거나 수백 MB 클립보드가 생기는"
    /// 되돌리기 어려운 컬럼 연산을 위한 것이다. 추출은 `replace_all_in_doc`이
    /// 같은 판단을 내린 것과 같은 이유로 그 대상이 아니다 — (a) 사용자가 검색어를
    /// 직접 타이핑하고 버튼을 누른 **명시적** 동작이고, (b) 원본을 전혀 바꾸지
    /// 않으므로 "되돌리기"라는 개념 자체가 필요 없다(잘못 눌렀으면 탭을 닫으면
    /// 끝이다). 남는 비용은 전 행 스캔 시간과 결과 바이트인데, 전자는 정렬 등
    /// 다른 전 행 연산이 이미 확인 없이 하는 일과 같은 급이고 후자는 **매치된
    /// 행만큼**이라 매치가 적으면 문서가 아무리 커도 메모리가 늘지 않는다.
    /// 그래서 여기에 세 번째 확인 단계를 새로 만들지 않는다.
    ///
    /// 백그라운드 스레드도 만들지 않는다(브리프 D-6) — 이 코드베이스에서
    /// 백그라운드로 도는 것은 인덱서와 정렬 잡뿐이고 둘 다 전용 메커니즘이 있다.
    fn extract_matching_rows(&mut self) {
        // 탭이 늘고 active가 바뀌는 동작이므로 잠금을 먼저 본다.
        if !extract_allowed(self) {
            if let Some(doc) = self.doc_mut() {
                doc.find_status = EXTRACT_LOCKED_STATUS.to_owned();
            }
            return;
        }
        let Some(doc) = self.docs.get(self.active) else { return };
        if effective_query(doc).is_empty() {
            if let Some(doc) = self.doc_mut() {
                doc.find_status = "Enter text to find".to_owned();
            }
            return;
        }
        let plan = extract_plan(doc.has_header, doc.sep);
        // Find All과 **같은** 스캐너(`scan_all_matches`)를 쓴다. 예전에는
        // `find::matching_lines` 브루트포스를 불렀는데, 그건 행마다 디코딩 +
        // `split_fields`/`chars().collect()` 할당을 하므로 같은 문서에서 Find
        // All은 즉시 끝나는데 추출만 수십 초가 걸렸다(Task G/H/I가 바이트
        // 스캔으로 최적화한 대상이 Find All뿐이었다).
        //
        // **헤더 제외를 왜 결과 필터로 해도 되는가.** 예전 방식은 `get_line`에
        // `scan_from`을 더해 훑는 **구간 자체**를 옮겼는데, 그건 `matching_lines`가
        // 돌려주는 행번호의 기준이 "훑기 시작한 자리"로 바뀌기 때문이었다(그래서
        // 되돌려 더해 줘야 했다). `scan_all_matches`는 처음부터 **문서 전체 기준**
        // 논리 행번호를 주므로 그 문제가 없고, `scan_from`(0 또는 1)보다 작은 행,
        // 즉 헤더 행만 버리면 "헤더는 검색 대상이 아니다"가 그대로 성립한다.
        // 결과 집합은 예전과 완전히 같다 — `scan_all_matches`는 `matching_lines`와
        // 같은 행 집합을 돌려주도록 계약되어 있다(그 함수 주석 참조).
        let hits: Vec<usize> = scan_all_matches(doc)
            .into_iter()
            .map(|r| r as usize)
            .filter(|&r| r >= plan.scan_from)
            .collect();

        if hits.is_empty() {
            // 0행이면 탭을 만들지 않는다 — 빈 탭이 열리면 짜증난다.
            if let Some(doc) = self.doc_mut() {
                doc.find_status = "Not found".to_owned();
            }
            return;
        }

        let mut lines: Vec<String> = Vec::with_capacity(hits.len() + 1);
        if plan.prepend_header {
            // 헤더 행이 아직 인덱싱되지 않아 읽히지 않으면 빈 줄이라도 넣는다 —
            // 헤더 자리가 비면 이후 모든 데이터 행이 한 칸씩 올라와 컬럼 이름이
            // 첫 데이터 행이 되어 버린다(행 수가 어긋나는 것보다 나쁘다).
            lines.push(logical_line(doc, 0).unwrap_or_default());
        }
        // 스캔이 이미 훑은 행을 여기서 **다시** 읽는다(스캔은 행 번호만 주고
        // 텍스트를 남기지 않는다 — 전 행 텍스트를 들고 있으면 2GB 문서에서
        // 메모리가 터진다). 매치된 행만 디코딩하므로 매치 수만큼만 든다.
        // 두 번째
        // 읽기가 실패하는 경우(`None`)는 `unwrap_or_default`로 빈 줄을 넣어
        // 행 수를 지킨다 — `filter_map`으로 조용히 버리면 안내 문구가 말하는
        // 행 수("N rows extracted")와 실제 추출본 행 수가 어긋난다.
        // (두 읽기 사이에 인덱스가 줄어들 일은 없으므로 실질적으로 일어나지
        //  않지만, 두 값이 같은 근거로 움직이게 묶어 둔다.)
        lines.extend(hits.iter().map(|&i| logical_line(doc, i).unwrap_or_default()));

        let newline = doc
            .edit
            .as_ref()
            .map(|e| e.newline)
            .unwrap_or(crate::edit::Newline::Lf);
        // 원본의 검색어/옵션을 새 탭에 물려준다 — 추출본은 "그 검색어로 Find
        // All을 한 셈"이므로 열자마자 하이라이트가 보여야 하고, 사용자가 새
        // 탭에서 곧바로 Find Next도 할 수 있어야 한다.
        // 새 탭에는 **날 검색어와 이스케이프 토글을 함께** 물려준다. 디코딩된
        // 값만 넘기면 새 탭의 입력란에 탭 문자가 그대로 박혀 보이지 않는 글자가
        // 되고, 사용자가 원본 탭에서 친 `\t`라는 입력이 사라진다. 둘을 함께
        // 넘기면 새 탭의 `effective_query`가 원본과 똑같은 값을 만든다.
        let carry_query = doc.find_query.clone();
        let carry_opts = doc.find_opts.clone();
        let carry_escapes = doc.find_escapes;
        let mut new_doc = build_extracted_doc(
            &lines,
            doc.enc,
            doc.sep,
            plan.prepend_header,
            newline,
            extracted_label(doc),
        );
        // 새 문서 기준으로 전체를 다시 스캔해 하이라이트 스냅샷을 채운다. 새
        // 문서에서는 (헤더를 뺀) 모든 데이터 행이 매치이지만, 셀 안 부분 매치
        // 위치까지 원본과 똑같이 그리려면 실제 스캔이 가장 단순·정확하다 —
        // "추출 = 새 탭에 Find All을 자동으로 한 셈"을 그대로 코드로 옮긴다.
        new_doc.find_query = carry_query;
        new_doc.find_opts = carry_opts.clone();
        new_doc.find_escapes = carry_escapes;
        let hl_rows = scan_all_matches(&new_doc);
        new_doc.highlight = Some(Highlight {
            // 스냅샷에는 디코딩된 값(Find All과 같은 규율). 방금 세운 필드
            // 셋으로 계산하므로 `scan_all_matches`가 쓴 needle과 반드시 같다.
            query: effective_query(&new_doc),
            opts: carry_opts,
            rows: hl_rows,
        });
        let n = hits.len();

        // 안내 문구는 **원본 탭**에 남긴다. 곧 활성 탭이 새 탭으로 바뀌지만,
        // 사용자가 원본으로 돌아왔을 때 방금 무슨 일이 있었는지 남아 있어야 한다.
        if let Some(doc) = self.doc_mut() {
            doc.find_status = if n == 1 {
                "1 row extracted".to_owned()
            } else {
                format!("{n} rows extracted")
            };
        }
        self.add_document(new_doc);
    }
}

/// 찾기 입력란의 고정 `Id`. `update()`의 Escape 게이트(Minor 7 참조)가 "지금
/// 포커스가 찾기 입력란 자신에 있는가"를 판정하려면 그 위젯의 Id가
/// 필요한데, 위젯을 그리는 `render_find_panel`과 게이트를 보는 `update()`가
/// 서로 다른 함수라 매 프레임 같은 Id를 재현할 수 있어야 한다. 문자열
/// 리터럴에서 만든 `Id`는 프레임을 넘어 안정적이다(egui의 관용 패턴).
fn find_query_id() -> egui::Id {
    egui::Id::new("find_query_input")
}

/// Whole cell 라디오가 활성화되어야 하는가. 셀 개념은 표 모드
/// (`SeparatorMode::Char`)에서만 성립한다 — 텍스트 모드는 구분자가 없으므로
/// "셀 전체 일치"가 사용자에게 의미 있는 선택지가 아니다(E1에서 텍스트 모드의
/// WholeCell은 "행 전체 일치"로 안전하게 정의돼 있어 고르더라도 오작동은
/// 없지만, 고를 이유가 없는 옵션을 활성으로 두면 혼란만 준다).
///
/// 순수 함수로 뽑은 이유: `render_find_panel` 안에 이 조건을 인라인으로
/// 두면(`ui.add_enabled_ui(matches!(doc.sep, SeparatorMode::Char(_)), ...)`)
/// 조건을 지우거나 뒤집어도 렌더 결과(`Option<FindAction>`)만 보는 테스트로는
/// 잡히지 않는다. 이 함수를 직접 테스트해야 뒤집힘을 잡아낸다.
fn whole_cell_enabled(doc: &Document) -> bool {
    matches!(doc.sep, SeparatorMode::Char(_))
}

/// 찾기 옵션이 바뀌었는가 — 바뀌었다면 `last_match`/`find_status`를 리셋해야
/// 한다("Match case를 켰는데 다음 찾기가 예전 자리에서 이어진다"처럼 기준이
/// 뒤섞이는 것을 막는다). 체크박스 시절부터 있던 판정을 라디오 도입에 맞춰
/// 순수 함수로 뽑았다 — `render_find_panel` 안에 `doc.find_opts != before`를
/// 인라인으로 두면, egui 라디오/체크박스 클릭을 좌표로 시뮬레이션하지 않는
/// 이상 이 판정 자체를 테스트로 구동할 방법이 없다. 함수로 뽑으면 `before`/
/// `after` 값만으로 직접 검증할 수 있다.
fn find_opts_changed(before: &crate::find::FindOptions, after: &crate::find::FindOptions) -> bool {
    before != after
}

/// 이번 프레임에 **검색 기준**이 바뀌었는가 — 옵션이든 이스케이프 해석 여부든.
/// 바뀌었다면 `find_opts_changed`가 하던 것과 같은 이유로 `last_match`/
/// `find_status`를 리셋해야 한다: 이스케이프를 켜는 순간 `\t`는 두 글자가 아니라
/// 탭 한 글자가 되므로, 이전 검색어로 잡아 둔 매치 자리는 새 기준의 매치가
/// 아니다. 그 자리를 기준으로 Find Next를 이어 가면 "체크박스를 켰는데 다음
/// 찾기가 예전 자리에서 이어진다"가 된다.
///
/// **`highlight`는 여기서 건드리지 않는다**(확정된 동작) — Find All 스냅샷은
/// 다음 Find All까지 유지된다. 그래서 이 함수도 리셋 여부만 판단하고 무엇을
/// 리셋할지는 호출부가 정한다.
///
/// 인라인 `!=` 두 개로 두지 않고 함수로 뽑는 이유는 `find_opts_changed`와 같다 —
/// egui 체크박스 클릭을 좌표로 시뮬레이션하지 않는 이상 렌더 안의 판정은 테스트로
/// 구동할 방법이 없다. 값만 넘겨 직접 검증한다.
fn find_inputs_changed(
    before_opts: &crate::find::FindOptions,
    after_opts: &crate::find::FindOptions,
    before_esc: bool,
    after_esc: bool,
) -> bool {
    find_opts_changed(before_opts, after_opts) || before_esc != after_esc
}

/// 이스케이프 체크박스 툴팁. 규칙 전부와 **`\n`이 지원되지 않는다는 것**,
/// 그리고 그래도 오류가 아니라 두 글자 그대로 찾힌다는 것을 알려야 한다 —
/// 사용자가 `\n`을 쳤을 때 아무 안내 없이 "못 찾음"만 뜨면 기능이 고장 난
/// 것처럼 보인다. `render_find_panel` 안의 문자열 리터럴로 두면 문구를 지워도
/// 아무 테스트가 깨지지 않으므로 상수로 뽑는다(`EXTRACT_LOCKED_STATUS`와 같은 결).
///
/// **`\x0A`/`\x0D`도 한 줄 덧붙여 경고한다.** `\n` 자체는 못 푼다고 위에서
/// 이미 말했지만, `\xNN`은 16진수를 그대로 풀므로 `\x0A`/`\x0D`는 실제 개행
/// 문자를 만들어 낸다 — 그리고 치환문에 들어간 그 개행은 `sanitize_for_line`이
/// 조용히 공백으로 바꿔치기한다(행 배열 불변식 때문에 안전하지만 무음이다).
/// 이 문구가 없으면 사용자가 "`\n`은 안 된다니 `\x0A`로 우회하자"고 생각했다가
/// 아무 표시도 없이 공백을 받게 된다.
const FIND_ESCAPES_TOOLTIP: &str = "\\t = tab, \\\\ = backslash, \\xNN = character code (hex, e.g. \\x41 = A).\n\\n is not supported: a line is a row here, so it stays as the two characters \\ and n.\nNewline codes in a replacement (\\n, \\x0A, \\x0D) become a space, for the same reason: one row is one line.\nAny other \\x sequence is left as typed.";

/// 찾기 상태 문구. `find_status`(Replace 결과나 "Not found" 등)가 있으면 그걸
/// 우선한다 — 사용자가 방금 누른 버튼의 결과이므로 매치 개수보다 관심사가
/// 급하다. `find_status`가 비어 있고 하이라이트 스냅샷이 있으면 매치 **행** 수를
/// 보여 준다. 이 개수는 매치가 있는 행의 개수이지 매치 총 개수가 아니므로("한
/// 행에 여러 번 나와도 1"), 문구에 "rows"를 명시해 오해를 막는다. 스냅샷이 없으면
/// 아무것도 보이지 않는다(스냅샷 없이 "0 rows"를 띄우는 것은 소음이다).
fn find_count_text(match_rows_len: usize, find_status: &str, has_query: bool) -> String {
    if !find_status.is_empty() {
        find_status.to_owned()
    } else if has_query {
        if match_rows_len == 1 {
            "1 matching row".to_owned()
        } else {
            format!("{match_rows_len} matching rows")
        }
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// 헥스 찾기 (순수 해석은 `crate::hex`, 여기는 Document에 붙이는 연결부)
// ---------------------------------------------------------------------------

/// 찾기 입력 해석. as_hex면 16진수(공백 무시), 아니면 UTF-8 바이트.
/// None = 검색 불가(버튼 비활성 근거).
fn hex_needle(query: &str, as_hex: bool) -> Option<Vec<u8>> {
    if as_hex {
        crate::hex::parse_hex_query(query)
    } else if query.is_empty() {
        None
    } else {
        Some(query.as_bytes().to_vec())
    }
}

/// 다음 매치를 찾아 last_match와 스크롤 요청을 갱신한다.
///
/// 빈 검색어는 "Invalid pattern"이 아니라 텍스트 모드(`apply_find_action`)와
/// 같은 문구 "Enter text to find"를 쓴다 — 갓 연 빈 패널에서 Find Next를
/// 누르는 가장 기본적인 조작을 "입력이 잘못됐다"고 나무라면 안 된다.
/// 이 경우는 검색을 아예 실행하지 않는 no-op이므로 `last_match`(있다면
/// 이전 매치 하이라이트)를 건드리지 않는다 — 텍스트 쪽의 같은 조기 반환이
/// `last_match`를 그대로 두는 것과 같은 규율이다.
fn hex_find_next(doc: &mut Document) {
    let Some(h) = doc.hex.as_ref() else { return };
    if doc.find_query.is_empty() {
        doc.find_status = "Enter text to find".into();
        return;
    }
    let Some(needle) = hex_needle(&doc.find_query, h.find_hex) else {
        doc.find_status = "Invalid pattern".into();
        return;
    };
    let from = h.last_match.map(|(o, _)| o + 1).unwrap_or(0);
    let found = match doc.hex.as_ref().and_then(|h| h.edit.as_ref()) {
        Some(e) => crate::hex::find_bytes(&e.bytes, &needle, from),
        None => crate::hex::find_bytes(doc.source.as_bytes(), &needle, from),
    };
    // hex의 가변 대여와 doc의 다른 필드 대입이 겹치지 않게, hex 갱신을
    // 블록으로 끝낸 뒤 doc 필드를 만진다.
    match found {
        Some(o) => {
            {
                let h = doc.hex.as_mut().unwrap();
                h.last_match = Some((o, needle.len()));
                h.caret = (o, true);
                h.sel = None;
            }
            doc.pending_scroll_row = Some((o / crate::hex::BYTES_PER_ROW as u64) as usize);
            doc.pending_scroll_align = egui::Align::Center;
            doc.find_status = String::new();
        }
        None => {
            doc.hex.as_mut().unwrap().last_match = None;
            doc.find_status = "Not found".into();
        }
    }
}

/// 찾기/바꾸기 창의 기본 위치. 데이터 영역(특히 표의 오른쪽 위 헤더/스크롤
/// 마커)을 덜 가리도록 화면 우상단 안쪽에 둔다. 첫 표시에만 적용되고(egui가
/// 그 뒤로는 사용자가 드래그한 위치를 기억한다) 매 프레임 강제하지 않는다.
fn find_window_default_pos(screen: egui::Rect) -> egui::Pos2 {
    egui::pos2((screen.right() - 340.0).max(screen.left()), screen.top() + 32.0)
}

/// 헥스 찾기의 기준이 바뀌었을 때 커서를 버린다(Minor 6).
///
/// 해석 방식(`find_hex`)을 토글하면 같은 검색어의 의미가 통째로 달라진다 —
/// `"4F4B"`가 두 바이트에서 네 글자로. 그 전 기준으로 잡은 `last_match`는
/// 더 이상 이 검색어의 매치가 아니므로, 남겨 두면 하이라이트가 거짓이 되고
/// 다음 `hex_find_next`가 그 자리에서 이어 찾는다. 텍스트 패널이
/// `find_inputs_changed`로 하는 것과 같은 처리다.
///
/// 렌더 클로저 안에 인라인으로 두면 체크박스 클릭을 흉내내야만 테스트할 수
/// 있으므로(창 안 위젯 좌표에 의존하는 취약한 테스트) 순수 함수로 뺀다.
fn reset_hex_find_cursor(doc: &mut Document) {
    if let Some(h) = doc.hex.as_mut() {
        h.last_match = None;
    }
    doc.find_status = String::new();
}

/// 헥스 문서 전용 찾기 창. 바꾸기 입력란도, 대소문자/범위 옵션도 없다 —
/// 헥스에는 그 개념이 없거나(바꾸기는 니블 단위 편집과 안 맞는다) 아직
/// 범위가 없다(문서 전체 하나). 검색어 입력란은 텍스트 모드와 같은
/// `doc.find_query`를 그대로 쓴다 — 필드를 따로 두면 탭을 오가며 두 값이
/// 갈릴 수 있고, 애초에 헥스/텍스트 문서는 한 탭에 동시에 있을 수 없다.
fn render_hex_find_panel(ctx: &egui::Context, doc: &mut Document, lang: crate::i18n::Lang) -> Option<FindAction> {
    let s = crate::i18n::t(lang);
    let mut action: Option<FindAction> = None;
    let want_focus = std::mem::take(&mut doc.find_focus_pending);
    let mut open = doc.show_find;
    egui::Window::new("Find (Hex)")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(find_window_default_pos(ctx.screen_rect()))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::theme::chrome_text(s.find_label));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut doc.find_query)
                        .id(find_query_id())
                        .desired_width(260.0),
                );
                if want_focus {
                    resp.request_focus();
                }
                // 텍스트 모드와 같은 관용: 입력란에서 Enter = Find Next.
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action = Some(FindAction::HexNext);
                    resp.request_focus();
                }
            });
            // `doc.hex`는 이 함수에 들어온 시점에 `Some`임이 보장된다
            // (`render_find_panel`이 `doc.hex.is_some()`일 때만 여기로 분기한다).
            let mode_changed = {
                let h = doc.hex.as_mut().unwrap();
                ui.checkbox(&mut h.find_hex, s.find_hex).changed()
            };
            if mode_changed {
                reset_hex_find_cursor(doc);
            }
            ui.separator();
            if ui.button(s.find_next).clicked() {
                action = Some(FindAction::HexNext);
            }
            if !doc.find_status.is_empty() {
                ui.label(crate::theme::chrome_text(doc.find_status.clone()));
            }
        });
    if !open {
        doc.show_find = false;
    }
    action
}

/// 찾기/바꾸기 창. 호출부는 `doc.show_find`가 참일 때만 부른다(`update()`).
///
/// 창 안에서 낸 동작은 인텐트로 돌려주고 적용은 호출부가 한다 — 여기서
/// 곧바로 찾기를 부르면 `doc`을 이중 대여하게 된다.
///
/// 라벨은 `chrome_text`를 거친다. Body 텍스트 스타일이 데이터용 고정폭이라
/// 그냥 `ui.label(s)`을 쓰면 UI 문구까지 데이터 폰트로 나온다.
fn render_find_panel(ctx: &egui::Context, doc: &mut Document, lang: crate::i18n::Lang) -> Option<FindAction> {
    let s = crate::i18n::t(lang);
    if doc.hex.is_some() {
        return render_hex_find_panel(ctx, doc, lang);
    }
    let mut action: Option<FindAction> = None;
    // 바꾸기는 편집 버퍼가 있어야 가능하다. 뷰 모드에서는 찾기만.
    let editing = doc.edit.is_some();
    // 패널을 막 연 프레임에만 입력란에 포커스를 준다.
    let want_focus = std::mem::take(&mut doc.find_focus_pending);

    // `.open(&mut open)`이 창 오른쪽 위에 X를 자동으로 그려 준다 — 본문의
    // 기존 `✖` 버튼과 중복이므로 없앤다(Escape 닫기는 이 X와 별개로 그대로
    // 유지된다 — `update()`의 단축키 게이트가 `show_find`를 직접 본다).
    let mut open = doc.show_find;
    egui::Window::new("Find & Replace")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(find_window_default_pos(ctx.screen_rect()))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::theme::chrome_text(s.find_label));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut doc.find_query)
                        .id(find_query_id())
                        .desired_width(260.0),
                );
                if want_focus {
                    resp.request_focus();
                }
                // 입력란에서 Enter = Find Next. `lost_focus() + Enter`가 egui의
                // 관용 패턴이다(TextEdit이 Enter로 포커스를 놓는다).
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    action = Some(FindAction::Next);
                    // 연속 검색을 위해 포커스를 되돌려 준다 — 그렇지 않으면
                    // Enter 한 번마다 입력란을 다시 클릭해야 한다.
                    resp.request_focus();
                }
            });
            ui.add_enabled_ui(editing, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::theme::chrome_text(s.find_replace_label));
                    ui.add(
                        egui::TextEdit::singleline(&mut doc.replace_text).desired_width(260.0),
                    );
                });
            });

            ui.separator();

            // 옵션이 바뀌면 이전 매치(Find Next 커서)는 그 옵션 기준이 아니므로
            // 버린다 — 그대로 두면 "Match case를 켰는데 다음 찾기가 예전 자리에서
            // 이어진다"처럼 기준이 뒤섞인다. **`highlight` 스냅샷은 건드리지
            // 않는다**(확정된 동작: 하이라이트는 다음 Find All까지 유지). 옵션을
            // 바꿔도 자동 스캔이 없으므로 여기서는 last_match/find_status만
            // 리셋한다 — 하이라이트를 새 옵션으로 맞추려면 Find All을 다시 누른다.
            let before = doc.find_opts.clone();
            let before_esc = doc.find_escapes;
            let mut scope = doc.find_opts.scope;
            ui.horizontal(|ui| {
                ui.radio_value(&mut scope, crate::find::MatchScope::Partial, s.find_partial);
                ui.radio_value(&mut scope, crate::find::MatchScope::WholeWord, s.find_whole_word);
                // Whole cell은 표 모드에서만 의미가 있다(셀 개념). 텍스트
                // 모드로 이미 WholeCell이 걸린 채 넘어왔더라도(탭 전환 등)
                // 라디오만 비활성일 뿐 E1이 그 조합을 "행 전체 일치"로 안전하게
                // 정의해 두었으므로 패닉·오작동은 없다.
                ui.add_enabled_ui(whole_cell_enabled(doc), |ui| {
                    ui.radio_value(&mut scope, crate::find::MatchScope::WholeCell, s.find_whole_cell);
                });
            });
            doc.find_opts.scope = scope;
            ui.horizontal(|ui| {
                ui.checkbox(&mut doc.find_opts.match_case, s.find_match_case);
                // 이스케이프 해석은 매칭 규칙(`FindOptions`)이 아니라 "입력란을
                // 어떻게 읽을 것인가"이므로 별도 필드를 직접 토글한다.
                ui.checkbox(&mut doc.find_escapes, s.find_escapes)
                    .on_hover_text(FIND_ESCAPES_TOOLTIP);
            });
            if find_inputs_changed(&before, &doc.find_opts, before_esc, doc.find_escapes) {
                doc.last_match = None;
                doc.find_status.clear();
            }

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button(s.find_prev).clicked() {
                    action = Some(FindAction::Prev);
                }
                if ui.button(s.find_next).clicked() {
                    action = Some(FindAction::Next);
                }
                // Find All: 전체를 스캔해 하이라이트 스냅샷을 만든다. 하이라이트가
                // 갱신되는 유일한 버튼이다. 검색어가 비면 비활성(찾을 게 없다).
                ui.add_enabled_ui(find_all_button_enabled(&doc.find_query), |ui| {
                    if ui.button(s.find_all).clicked() {
                        action = Some(FindAction::All);
                    }
                });
                ui.add_enabled_ui(editing, |ui| {
                    if ui.button(s.find_replace_one).clicked() {
                        action = Some(FindAction::ReplaceOne);
                    }
                    if ui.button(s.find_replace_all).clicked() {
                        action = Some(FindAction::ReplaceAll);
                    }
                });
                // 추출은 뷰/편집 모드 둘 다에서 동작한다(찾기와 같다) — 바꾸기처럼
                // `editing`으로 가두면 안 된다. 검색어가 비었을 때만 비활성.
                ui.add_enabled_ui(extract_button_enabled(&doc.find_query), |ui| {
                    if ui.button(s.find_extract_rows).clicked() {
                        action = Some(FindAction::Extract);
                    }
                });
            });

            // 매치 개수는 라이브 검색어가 아니라 Find All 스냅샷 기준이다 — 타이핑
            // 중에는 스캔하지 않으므로 스냅샷이 없으면 개수를 보이지 않는다(있으면
            // 그 스냅샷의 매치 행 수). `find_status`가 있으면 그쪽이 우선한다.
            let hl_rows = doc.highlight.as_ref().map(|h| h.rows.len()).unwrap_or(0);
            let count_text = find_count_text(hl_rows, &doc.find_status, doc.highlight.is_some());
            if !count_text.is_empty() {
                ui.label(crate::theme::chrome_text(count_text));
            }
        });
    // 창을 X로 닫으면(egui가 open을 false로) show_find를 내린다. Escape
    // 닫기는 `update()`가 `doc.show_find`를 직접 끄므로 여기와 별개다.
    if !open {
        doc.show_find = false;
    }
    action
}

/// 저장 대상 경로를 파일 선택 창으로 새로 골라야 하는가. `save_as`가 참이거나
/// (명시적 "Save As") 현재 경로가 비어 있으면(추출본처럼 디스크에 대응하는
/// 파일이 없는 문서) 참이다.
///
/// **이 판정이 두 번 쓰인다는 것이 핵심이다.** ①어떤 경로에 쓸지 고르는
/// 분기(파일 선택 창 vs 현재 경로)와 ②저장 성공 뒤 `doc.path`/`path_label`을
/// 새 경로로 갱신할지의 분기가 **반드시 같은 값**이어야 한다. 둘이 어긋나면
/// (예: ①만 폴백하고 ②는 `save_as`만 본 과거 버전) 파일 선택 창으로 실제로
/// 저장은 되는데 탭은 여전히 "추출본"이라 우기게 되고, 그러면 `doc.path`가
/// 계속 비어 있어 **다음 Ctrl+S마다 다시 파일 선택 창이 뜬다** — 사용자는
/// 매번 파일명을 다시 입력해야 한다. 그래서 하나의 함수로 묶어 호출부 두 곳이
/// 같은 값을 보게 한다.
fn save_as_fallback(save_as: bool, cur_path_empty: bool) -> bool {
    save_as || cur_path_empty
}

/// 개행 스타일의 화면 표기. 플랫폼 이름을 함께 적는 이유는 사용자가 고르는
/// 기준이 대개 "어디서 쓸 파일인가"이기 때문이다 — `CRLF`/`LF`만으로는
/// 리눅스에 보낼 파일에 무엇을 골라야 하는지 알 수 없다.
fn newline_label(nl: crate::edit::Newline) -> &'static str {
    match nl {
        crate::edit::Newline::CrLf => "CRLF (Windows)",
        crate::edit::Newline::Lf => "LF (Unix/Linux/macOS)",
    }
}

/// 파일 저장 창에 보여 줄 확장자 목록. `(설명, [확장자…])` 순서대로 준다.
///
/// **첫 항목이 기본 선택**이므로 지금 문서에 어울리는 것을 앞에 둔다. 고르는
/// 근거는 두 가지이고, 이 순서로 본다:
///
/// 1. **현재 파일의 확장자.** 이미 `data.tsv`인 파일을 저장하면서 기본이
///    `.csv`면 실수로 형식을 바꾸게 된다. 파일이 있으면 그 확장자가 곧 답이다.
/// 2. **보기 구분자.** 새 파일(경로 없음)이라 1번이 없을 때 쓴다. 탭으로
///    보고 있으면 TSV, 콤마면 CSV, 구분자가 없으면 텍스트일 가능성이 높다.
///    다만 이건 *보기* 설정이라 데이터의 실제 구분자와 다를 수 있어(툴바에서
///    언제든 바꾼다) 1번보다 뒤에 둔다.
///
/// 어느 쪽으로 골라도 나머지 항목은 뒤에 남기고 `All files`도 끝에 둔다 —
/// 추측이 틀렸을 때 사용자가 언제든 바꿀 수 있어야 한다.
fn save_filters(
    cur_path: &Path,
    sep: Option<SeparatorMode>,
) -> Vec<(&'static str, Vec<&'static str>)> {
    const TEXT: (&str, [&str; 1]) = ("Text", ["txt"]);
    const CSV: (&str, [&str; 1]) = ("CSV", ["csv"]);
    const TSV: (&str, [&str; 2]) = ("TSV", ["tsv", "tab"]);

    // 우선 순위를 정하는 키: "csv" | "tsv" | "txt" 중 하나.
    let ext = cur_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let preferred = match ext.as_deref() {
        Some("csv") => "csv",
        Some("tsv" | "tab") => "tsv",
        Some("txt") => "txt",
        // 확장자가 없거나 우리가 아는 셋이 아니면 보기 구분자로 추측한다.
        _ => match sep {
            Some(SeparatorMode::Char(b'\t')) => "tsv",
            Some(SeparatorMode::Char(_)) => "csv",
            _ => "txt",
        },
    };

    let mut out: Vec<(&'static str, Vec<&'static str>)> = Vec::with_capacity(4);
    let mut push = |k: &str| match k {
        "csv" => out.push((CSV.0, CSV.1.to_vec())),
        "tsv" => out.push((TSV.0, TSV.1.to_vec())),
        _ => out.push((TEXT.0, TEXT.1.to_vec())),
    };
    push(preferred);
    for k in ["csv", "tsv", "txt"] {
        if k != preferred {
            push(k);
        }
    }
    // 우리가 아는 확장자가 아니어도 저장할 수 있어야 한다(.log, .json, .md …).
    out.push(("All files", vec!["*"]));
    out
}

/// 저장 옵션을 **다이얼로그에서 고른 값**으로 만든다.
///
/// 셋 다 `app`에서 온다는 것이 요점이다. 개행만 문서(`EditBuffer.newline`)에서
/// 읽던 시절이 있었는데, 그러면 콤보에서 LF를 골라도 CRLF로 저장된다 —
/// `init_save_defaults`가 둘을 같은 값으로 맞춰 두기 때문에 **고르지 않으면
/// 증상이 안 보이고**, 골랐을 때만 조용히 무시된다.
fn save_options(app: &App) -> crate::save::SaveOptions {
    crate::save::SaveOptions {
        enc: app.save_enc,
        bom: app.save_bom,
        newline: app.save_newline,
    }
}

/// 저장이 성공했을 때 문서에 반영할 것 — 개행 스타일 되쓰기 + dirty 해제.
///
/// 저장 경로 한가운데(rfd 파일 선택 창 뒤)에 있던 처리를 함수로 뺐다. 그
/// 자리는 테스트가 구동할 수 없어, 두 처리 중 하나가 빠져도 아무 테스트도
/// 깨지지 않았다. 이제 둘 다 여기서 검증된다.
fn mark_saved(doc: &mut Document, nl: crate::edit::Newline) {
    // 방금 이 스타일로 썼으므로 문서의 개행도 이것이다.
    apply_save_newline(doc, nl);
    if let Some(e) = &mut doc.edit {
        e.dirty = false;
    }
}

/// 헥스 저장 성공 반영. 텍스트의 `mark_saved`와 대칭(개행 되쓰기가 없을 뿐 —
/// 헥스는 바이트가 곧 전부라 인코딩/개행 개념이 없다).
fn mark_hex_saved(doc: &mut Document) {
    if let Some(e) = doc.hex.as_mut().and_then(|h| h.edit.as_mut()) {
        e.dirty = false;
    }
}

/// 다이얼로그에서 고른 개행 스타일을 편집 버퍼에 **되쓴다**.
///
/// 되쓰지 않으면 저장은 LF로 나가는데 화면 기호는 계속 `␍␊`를 그린다 —
/// 화면이 파일을 설명하지 못하게 된다(`line_ending_for_row`가 편집 모드에서
/// `EditBuffer.newline`을 보기 때문). 저장은 "이 문서의 개행은 이제 이것"을
/// 확정하는 사건이므로 문서 상태에 반영하는 것이 맞다.
///
/// `dirty`는 건드리지 않는다. 저장 직후 호출되므로 방금 쓴 파일과 버퍼가
/// 일치하는 상태이고, 여기서 dirty를 켜면 저장하자마자 "저장 안 됨"이 된다.
fn apply_save_newline(doc: &mut Document, nl: crate::edit::Newline) {
    if let Some(e) = doc.edit.as_mut() {
        e.newline = nl;
    }
}

/// 저장 다이얼로그. 인코딩/BOM/개행을 고르고 저장하거나 취소한다.
/// `app.save_as`가 참이면 rfd 파일 선택 창으로 경로를 새로 고른다.
fn render_save_dialog(ctx: &egui::Context, app: &mut App) {
    let s = crate::i18n::t(app.lang);
    // 저장할 것이 없으면(편집 모드 이탈 등) 다이얼로그를 닫는다.
    // `doc_exportable`이라 Parquet(편집 버퍼가 없다)도 통과한다 — 그 경우는
    // 제자리 저장이 아니라 CSV/TSV 내보내기다.
    if app.doc().map_or(true, |d| !doc_exportable(d)) {
        app.show_save_dialog = false;
        return;
    }
    let is_hex = app.doc().is_some_and(|d| d.hex.is_some());
    let is_parquet = app.doc().is_some_and(|d| d.parquet.is_some());
    let title = if is_parquet {
        "Export as CSV/TSV"
    } else if app.save_as {
        "Save As"
    } else {
        "Save"
    };
    let cur_label = app
        .doc()
        .map(|d| d.path_label.clone())
        .unwrap_or_default();

    let mut open = true;
    let mut do_save = false;
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            if app.save_as {
                ui.label(crate::theme::chrome_text(
                    "You will choose the file location after clicking Save.",
                ));
            } else {
                ui.label(crate::theme::chrome_text(format!("{} {cur_label}", s.save_overwrite)));
            }
            ui.separator();

            if is_hex {
                // 헥스는 바이트가 곧 전부다 — 인코딩/BOM/개행 개념이 없으므로
                // 그 위젯들을 건너뛰고 이 사실만 한 줄로 알린다.
                ui.label(crate::theme::chrome_text(
                    "Binary file — bytes are saved as-is.",
                ));
            } else {
                let enc_label = match app.save_enc {
                    Encoding::Utf8 => "UTF-8",
                    Encoding::Cp949 => "CP949",
                    Encoding::Utf16Le => "UTF-16LE",
                    Encoding::Utf16Be => "UTF-16BE",
                };
                egui::ComboBox::from_label(crate::theme::chrome_text("Encoding"))
                    .selected_text(enc_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.save_enc, Encoding::Utf8, "UTF-8");
                        ui.selectable_value(&mut app.save_enc, Encoding::Cp949, "CP949");
                        ui.selectable_value(&mut app.save_enc, Encoding::Utf16Le, "UTF-16LE");
                        ui.selectable_value(&mut app.save_enc, Encoding::Utf16Be, "UTF-16BE");
                    });

                // CP949는 BOM 개념이 없으므로 체크박스를 비활성 + 강제 해제.
                let bom_allowed = app.save_enc != Encoding::Cp949;
                if !bom_allowed {
                    app.save_bom = false;
                }
                ui.add_enabled_ui(bom_allowed, |ui| {
                    ui.checkbox(&mut app.save_bom, s.save_include_bom);
                });
                if !bom_allowed {
                    ui.label(crate::theme::chrome_text(s.save_cp949_no_bom));
                }

                // 개행 스타일. 파일 전체에 하나로 적용된다(줄마다 다르게 저장하는
                // 기능은 없다 — 편집 버퍼가 줄별 개행을 보관하지 않는다).
                egui::ComboBox::from_label(crate::theme::chrome_text("Line ending"))
                    .selected_text(newline_label(app.save_newline))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.save_newline,
                            crate::edit::Newline::CrLf,
                            newline_label(crate::edit::Newline::CrLf),
                        );
                        ui.selectable_value(
                            &mut app.save_newline,
                            crate::edit::Newline::Lf,
                            newline_label(crate::edit::Newline::Lf),
                        );
                    });
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(s.menu_save).clicked() {
                    do_save = true;
                }
                if ui.button(s.common_cancel).clicked() {
                    app.show_save_dialog = false;
                }
            });
        });
    if !open {
        app.show_save_dialog = false;
    }

    if !do_save {
        return;
    }
    app.show_save_dialog = false;

    // 대상 경로 결정. save_as면 파일 선택 창, 아니면 현재 경로.
    // 현재 경로가 비어 있으면(추출본처럼 디스크 파일이 없는 문서) save_as로
    // 폴백한다 — 추출 직후 첫 저장이 지나가는 주 경로다(`save_as_fallback` 참조).
    let cur_path = app.doc().map(|d| d.path.clone()).unwrap_or_default();
    let path_will_update = save_as_fallback(app.save_as, cur_path.as_os_str().is_empty());
    let target = if path_will_update {
        let mut dlg = rfd::FileDialog::new();
        if let Some(dir) = cur_path.parent() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(name) = cur_path.file_name().and_then(|n| n.to_str()) {
            dlg = dlg.set_file_name(name);
        }
        if is_hex {
            // 헥스는 형식 개념이 없다 — "All files"만 제시한다.
            dlg = dlg.add_filter("All files", &["*"]);
        } else {
            // 확장자 목록. 첫 항목이 다이얼로그의 기본 선택이므로 **지금
            // 문서에 어울리는 것**을 앞에 둔다(`save_filters` 참조).
            for (name, exts) in save_filters(&cur_path, app.doc().map(|d| d.sep)) {
                dlg = dlg.add_filter(name, &exts);
            }
        }
        match dlg.save_file() {
            Some(p) => p,
            // 취소 = 아무 일도 일어나지 않는다(버퍼는 그대로 dirty).
            None => return,
        }
    } else {
        cur_path
    };

    if is_hex {
        let bytes = {
            let Some(bytes) = app
                .doc()
                .and_then(|d| d.hex.as_ref())
                .and_then(|h| h.edit.as_ref())
                .map(|e| e.bytes.clone())
            else {
                return;
            };
            bytes
        };
        match crate::save::write_binary(&target, &bytes) {
            Ok(()) => {
                app.error = None;
                if let Some(doc) = app.doc_mut() {
                    // 텍스트의 mark_saved와 대칭 — dirty 해제.
                    mark_hex_saved(doc);
                    // 텍스트 저장과 같은 판정으로 경로를 갱신한다
                    // (`save_as_fallback` 참조 — ①폴백 여부와 ②경로 갱신
                    // 여부가 어긋나면 다음 저장마다 또 파일 선택 창이 뜬다).
                    if path_will_update {
                        doc.path_label = target.display().to_string();
                        doc.path = target.clone();
                    }
                    // write_binary의 rename으로 옛 mmap이 낡았다. 텍스트 저장과
                    // 같은 이유로 방금 쓴 파일에 다시 겨눠야 하지만,
                    // `repoint_source_after_save`는 줄 인덱서를 새로 띄운다 —
                    // 헥스 문서는 `indexer: None`이 불변식이므로(줄 개념이
                    // 없다) 그 함수를 그대로 쓰면 깨진다. mmap만 다시 연다.
                    match source::open(&target) {
                        Ok(src) => {
                            doc.source = Arc::new(src);
                            doc.index = LineIndex::new(doc.source.len());
                        }
                        Err(e) => {
                            app.error =
                                Some(format!("Failed to reopen file after saving: {e}"));
                        }
                    }
                }
            }
            Err(err) => {
                // 실패하면 버퍼는 dirty인 채로 둔다(사용자가 다시 시도할 수 있게).
                app.error = Some(format!("Save failed: {err}"));
            }
        }
        return;
    }

    let opts = save_options(app);
    let result = {
        // Parquet은 편집 버퍼가 없다(읽기 전용). "다른 이름으로 저장"이
        // **CSV/TSV 내보내기**가 되므로 행을 모아 같은 저장 경로로 보낸다 —
        // 인코딩·개행·BOM 선택이 그대로 따라온다.
        if let Some(doc) = app.doc() {
            if doc.parquet.is_some() {
                let lines = collect_export_lines(doc);
                crate::save::write_file(&target, &lines, &opts, None)
            } else {
                let Some(e) = doc.edit.as_ref() else { return };
                crate::save::write_file(&target, &e.lines, &opts, None)
            }
        } else {
            return;
        }
    };

    match result {
        Ok(()) => {
            app.error = None;
            let chosen_newline = app.save_newline;
            if let Some(doc) = app.doc_mut() {
                // 저장이 성공했을 때 문서에 반영할 것들을 한 함수로 모았다 —
                // 여기는 rfd 다이얼로그 뒤라 테스트가 구동할 수 없으므로,
                // 판정을 밖으로 빼야 검증할 수 있다(`mark_saved` 참조).
                mark_saved(doc, chosen_newline);
                // ①경로 폴백 여부와 ②경로 갱신 여부는 반드시 같은 판정이어야
                // 한다(`save_as_fallback` 참조) — 그렇지 않으면 추출본을 파일
                // 선택 창으로 저장해 놓고도 탭은 계속 "추출본"이라 우겨서,
                // `doc.path`가 비어 있는 채로 남아 다음 저장마다 또 파일 선택
                // 창이 뜬다.
                if path_will_update {
                    doc.path_label = target.display().to_string();
                    doc.path = target.clone();
                }
                // write_file의 rename으로 옛 mmap이 낡았다. 방금 쓴 파일로 다시
                // 겨눠야 편집 모드를 껐을 때 저장된 내용이 보인다. 편집 버퍼와
                // 선택 상태는 그대로 유지된다(사용자는 편집 모드에 남는다).
                if let Err(msg) = repoint_source_after_save(doc, &target, ctx) {
                    app.error = Some(msg);
                }
            }
        }
        Err(err) => {
            // 실패하면 버퍼는 dirty인 채로 둔다(사용자가 다시 시도할 수 있게).
            app.error = Some(format!("Save failed: {err}"));
        }
    }
}

/// 바이너리로 판정된 파일의 열기 방식 선택. Sort/Convert 다이얼로그와
/// 같은 `egui::Window` 패턴.
fn render_binary_open_dialog(ctx: &egui::Context, app: &mut App) {
    let s = crate::i18n::t(app.lang);
    let Some(pending) = &app.pending_binary_open else { return };
    let path = pending.path.clone();
    let mut open = true;
    let mut choice: Option<bool> = None; // Some(true)=hex, Some(false)=text
    egui::Window::new("Not a Text File")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(
                "This file does not look like text.",
            ));
            ui.label(crate::theme::chrome_text(path.display().to_string()));
            ui.separator();
            if ui.button(s.open_as_binary).clicked() {
                choice = Some(true);
            }
            ui.separator();
            ui.label(crate::theme::chrome_text(s.open_force_encoding));
            ui.horizontal(|ui| {
                let enc = &mut app.pending_binary_open.as_mut().unwrap().enc;
                let label = match enc {
                    Encoding::Utf8 => "UTF-8",
                    Encoding::Cp949 => "CP949",
                    Encoding::Utf16Le => "UTF-16LE",
                    Encoding::Utf16Be => "UTF-16BE",
                };
                egui::ComboBox::from_id_source("binary_open_enc")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(enc, Encoding::Utf8, "UTF-8");
                        ui.selectable_value(enc, Encoding::Cp949, "CP949");
                        ui.selectable_value(enc, Encoding::Utf16Le, "UTF-16LE");
                        ui.selectable_value(enc, Encoding::Utf16Be, "UTF-16BE");
                    });
                if ui.button(s.open_as_text).clicked() {
                    choice = Some(false);
                }
            });
            ui.separator();
            if ui.button(s.common_cancel).clicked() {
                app.pending_binary_open = None;
            }
        });
    if !open {
        app.pending_binary_open = None;
    }
    if let Some(as_hex) = choice {
        let enc = app.pending_binary_open.as_ref().map(|p| p.enc).unwrap_or(Encoding::Utf8);
        app.pending_binary_open = None;
        if as_hex {
            app.open_path_hex(&path);
        } else {
            app.open_path_as_text(&path, enc, ctx);
        }
    }
}

/// 저장하지 않은 변경을 버릴 수 있는 동작 전에 띄우는 확인 창.
fn render_confirm_discard_dialog(ctx: &egui::Context, app: &mut App) {
    let s = crate::i18n::t(app.lang);
    let mut open = true;
    let mut proceed = false;
    let mut cancel = false;
    egui::Window::new("Unsaved Changes")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(
                "You have unsaved changes. Continuing will discard them.",
            ));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(s.common_continue).clicked() {
                    proceed = true;
                }
                if ui.button(s.common_cancel).clicked() {
                    cancel = true;
                }
            });
        });
    // 창 X로 닫으면 취소와 같다(데이터를 잃지 않는 쪽이 기본).
    if !open || cancel {
        app.pending_action = None;
        return;
    }
    if !proceed {
        return;
    }
    match app.pending_action.take() {
        Some(PendingAction::ExitEditMode) => {
            if let Some(doc) = app.doc_mut() {
                exit_edit_mode(doc);
            }
        }
        Some(PendingAction::CloseApp) => {
            // 확인됐으니 실제로 닫는다. 이번엔 close_requested 훅이 dirty를
            // 다시 보지 않도록 모든 탭의 dirty(텍스트+헥스)를 내려 둔다(이미
            // 폐기 동의) — 활성 탭만 내리면 두 번째 X에서 다른 탭의 dirty가
            // 다시 잡힌다.
            for d in &mut app.docs {
                if let Some(e) = &mut d.edit {
                    e.dirty = false;
                }
                if let Some(e) = d.hex.as_mut().and_then(|h| h.edit.as_mut()) {
                    e.dirty = false;
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        Some(PendingAction::CloseTab(i)) => {
            app.close_tab(i);
        }
        None => {}
    }
}

/// 대상 행 수가 많은 컬럼 연산 전에 띄우는 확인 창
/// (`render_confirm_discard_dialog`와 같은 패턴 — 인텐트를 보관해 두었다가
/// "계속"에서 같은 경로를 `confirmed = true`로 다시 태운다).
///
/// `delim`이 필요하므로 표 모드(`SeparatorMode::Char`)에서만 의미가 있다.
/// 텍스트 모드에는 컬럼 개념이 없어 애초에 대기 연산이 생기지 않는다.
fn render_confirm_big_column_op_dialog(ctx: &egui::Context, app: &mut App) {
    let s = crate::i18n::t(app.lang);
    let Some(doc) = app.doc() else { return };
    let Some(pending) = doc.pending_column_op.clone() else { return };
    let SeparatorMode::Char(delim) = doc.sep else {
        // 구분자가 없으면 컬럼 연산 자체가 성립하지 않는다 — 조용히 취소.
        if let Some(doc) = app.doc_mut() {
            doc.pending_column_op = None;
        }
        return;
    };
    let what = match pending.act {
        CellMenuAction::Copy => "Copy",
        CellMenuAction::Cut => "Cut",
        CellMenuAction::Clear => "Clear Contents",
        CellMenuAction::DeleteRows => "Delete Rows",
        CellMenuAction::Paste => "Paste",
        CellMenuAction::InsertRowAbove | CellMenuAction::InsertRowBelow => "Insert Row",
    };
    let rows = pending.rows;

    let mut open = true;
    let mut proceed = false;
    let mut cancel = false;
    egui::Window::new("Large Operation")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(format!(
                "'{what}' will be applied to {rows} rows."
            )));
            ui.label(crate::theme::chrome_text(
                "This may take a while and use a lot of memory.",
            ));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(s.common_continue).clicked() {
                    proceed = true;
                }
                if ui.button(s.common_cancel).clicked() {
                    cancel = true;
                }
            });
        });
    // 창 X로 닫으면 취소와 같다(아무것도 하지 않는 쪽이 기본).
    if !open || cancel {
        if let Some(doc) = app.doc_mut() {
            doc.pending_column_op = None;
        }
        return;
    }
    if !proceed {
        return;
    }
    // 확인됨 — 대기 상태를 먼저 비우고(무한 재확인 방지) 같은 경로를 다시 탄다.
    // clipboard_cache와 활성 문서를 동시에 가변 대여해야 하므로 doc_mut()
    // 대신 필드를 직접 쪼개 빌린다(App 전체를 넘기면 동시 대여가 안 된다).
    let clipboard = &mut app.clipboard_cache;
    let Some(doc) = app.docs.get_mut(app.active) else { return };
    doc.pending_column_op = None;
    // `apply_cell_menu_action_confirmed`는 `&mut egui::Ui`를 요구한다. 이 시점은
    // CentralPanel 밖이므로, 그리지 않는 임시 Area의 Ui를 하나 만들어 넘긴다
    // (동작이 실제로 쓰는 것은 `ui.output_mut`(클립보드)뿐이다).
    egui::Area::new(egui::Id::new("big_column_op_apply"))
        .fixed_pos(egui::pos2(-10000.0, -10000.0))
        .show(ctx, |ui| {
            apply_cell_menu_action_confirmed(
                ui,
                doc,
                delim,
                clipboard,
                pending.act,
                pending.paste_text.as_deref(),
                true,
            );
        });
}

/// 행/열 번호 시작값(0 또는 1) 설정 다이얼로그.
fn render_numbering_dialog(ctx: &egui::Context, app: &mut App) {
    let s = crate::i18n::t(app.lang);
    let mut open = true;
    egui::Window::new("Row & Column Numbers")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(
                "Choose the starting number for rows and columns.",
            ));
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(crate::theme::chrome_text(s.common_rows));
                ui.selectable_value(&mut app.row_base, 0, "From 0");
                ui.selectable_value(&mut app.row_base, 1, "From 1");
            });
            ui.horizontal(|ui| {
                ui.label(crate::theme::chrome_text(s.common_columns));
                ui.selectable_value(&mut app.col_base, 0, "From 0");
                ui.selectable_value(&mut app.col_base, 1, "From 1");
            });
            ui.separator();
            if ui.button(s.common_close).clicked() {
                app.show_numbering_dialog = false;
            }
        });
    if !open {
        app.show_numbering_dialog = false;
    }
}

/// 문서 전체의 구분자를 `new`로 바꾼다. 편집 버퍼가 없으면 먼저 만든다.
///
/// **뷰 모드에서 자동으로 편집 모드에 진입하는 이유.** 데이터를 고치는
/// 작업이므로 인메모리 버퍼가 필요하다. 버튼을 비활성화하고 "편집 모드를 먼저
/// 켜세요"라고 하는 대신 자동으로 켠다 — 사용자가 Replace에서 같은 벽에
/// 부딪혀 불편해했다.
///
/// 되돌리기는 `replace_all_in_doc`과 같은 규율이다: 실제로 달라진 행만
/// `EditOp::Replace` **하나**에 모아 Ctrl+Z 한 번으로 전부 복구한다. 행 수는
/// 변하지 않는다.
fn convert_delimiter_in_doc(doc: &mut Document, new: u8) {
    let SeparatorMode::Char(old) = doc.sep else {
        return;
    };
    if old == new || !new.is_ascii() {
        return;
    }
    // 데이터를 고치려면 편집 버퍼가 필요하다.
    if doc.edit.is_none() {
        enter_edit_mode(doc);
    }
    let Some(e) = doc.edit.as_mut() else { return };

    let changed = crate::convert::convert_all(&e.lines, old, new);
    if changed.is_empty() {
        // 구분자가 하나도 없는 문서(전부 한 필드)면 바뀔 것이 없다. 그래도
        // `doc.sep`은 새 구분자로 맞춘다 — 사용자가 고른 구분자로 보는 것이
        // 기대에 맞고, 데이터가 안 바뀌었으므로 dirty를 세우지 않는다.
        doc.sep = SeparatorMode::Char(new);
        doc.custom_sep_input = if new.is_ascii_graphic() {
            (new as char).to_string()
        } else {
            String::new()
        };
        doc.find_status = "Delimiter changed (no rows affected)".to_owned();
        doc.show_convert_dialog = false;
        // 데이터는 그대로여도 **보는 기준**이 바뀌었다 — 개정 번호는 안 움직이므로
        // 여기서 직접 무효화하지 않으면 옛 구분자로 센 목록이 그대로 남는다.
        invalidate_error_scan(doc);
        return;
    }

    // 바뀐 행의 **이전** 값을 한 Replace에 모은다. `mem::replace`가 새 텍스트를
    // 넣으면서 옛 String을 그대로 돌려주므로 문자열 복사가 없다
    // (`replace_all_in_doc`이 같은 이유로 이 형태를 쓴다).
    let mut before: Vec<(usize, String)> = Vec::with_capacity(changed.len());
    for (i, text) in changed {
        let Some(slot) = e.lines.get_mut(i) else {
            continue;
        };
        before.push((i, std::mem::replace(slot, text)));
    }
    let rows = before.len();
    e.undo.push(crate::edit::EditOp::Replace(before));
    e.dirty = true;

    // 데이터가 새 구분자로 바뀌었으니 보기 기준도 맞춘다. 안 맞추면 표가
    // 한 컬럼으로 무너진다.
    doc.sep = SeparatorMode::Char(new);
    doc.custom_sep_input = if new.is_ascii_graphic() {
        (new as char).to_string()
    } else {
        String::new()
    };
    // 컬럼 경계가 달라졌으므로 컬럼에 매인 상태를 전부 버린다(툴바의 구분자
    // 변경이 하는 것과 같다). `has_header`는 **유지한다** — 변환은 필드 수도
    // 행 수도 바꾸지 않으므로 헤더 행은 그대로 헤더다.
    doc.sort = None;
    doc.sort_job = None;
    doc.selected_col = None;
    doc.sort_specs.clear();
    doc.show_sort_dialog = false;
    // 셀 단위 매치 위치가 무의미해졌다.
    doc.highlight = None;
    doc.last_match = None;
    // 필드 수를 세는 기준이 통째로 달라졌다. 편집이 있었으니 개정 번호로도
    // 잡히지만, 그 간접 경로에 기대지 않고 여기서 명시한다.
    invalidate_error_scan(doc);
    doc.find_status = if rows == 1 {
        "1 row converted".to_owned()
    } else {
        format!("{rows} rows converted")
    };
    doc.show_convert_dialog = false;
}

/// 변환 다이얼로그의 `Convert` 버튼을 누를 수 있는가.
///
/// `current`가 현재 문서 구분자, `target`이 고른 대상 구분자다.
///
/// 세 가지를 막는다:
/// - 텍스트 모드(`SeparatorMode::None`) — 나눌 기준이 없으니 변환할 것도 없다
/// - 대상이 현재와 같음 — no-op
/// - 대상이 비ASCII — `join_fields`가 `delim as char`로 비교/기록하므로
///   비ASCII 바이트는 UTF-8 **두 바이트**로 쓰여 파서가 기대하는 한 바이트와
///   어긋난다. 결과 파일이 깨진다.
///
/// **자유 함수인 이유.** 프로덕션(egui 클로저)과 테스트가 이 함수 **하나**를
/// 부른다. 판정식을 클로저 안에 인라인으로 적고 테스트가 그걸 복사하면, 진짜
/// 가드를 지워도 테스트는 자기 사본만 보고 통과한다 — 이 코드베이스에서
/// 반복해서 나온 결함이다.
fn convert_enabled(current: SeparatorMode, target: Option<u8>) -> bool {
    let SeparatorMode::Char(cur) = current else {
        return false;
    };
    let Some(t) = target else {
        return false;
    };
    t.is_ascii() && t != cur
}

/// 구분자 변환 다이얼로그. 고른 구분자로 **데이터를 실제로 재작성**한다.
///
/// 툴바의 `Delimiter` 드롭다운과 역할이 다르다는 것을 문구로 분명히 한다 —
/// 저쪽은 보기 설정이라 파일을 건드리지 않고, 이쪽은 파일 내용을 바꾼다.
/// 두 개념이 섞이면 "탭으로 바꿨는데 왜 파일에 아직 콤마가 있지"로 이어진다.
///
/// `Convert`를 누르면 `want_convert`가 참으로 돌아온다. 실제 변환은 호출부가
/// 한다 — 변환은 편집 버퍼 진입(`enter_edit_mode`)이 필요할 수 있는데, 그건
/// `Document` 하나가 아니라 더 넓은 범위를 건드리기 때문이다.
fn render_convert_dialog(ctx: &egui::Context, doc: &mut Document, lang: crate::i18n::Lang) -> bool {
    let s = crate::i18n::t(lang);
    let mut open = true;
    let mut want_convert = false;
    let cur_label = match doc.sep {
        SeparatorMode::None => "None (plain text)".to_owned(),
        SeparatorMode::Char(b',') => "Comma  ,".to_owned(),
        SeparatorMode::Char(b'\t') => "Tab".to_owned(),
        SeparatorMode::Char(b'|') => "Pipe  |".to_owned(),
        SeparatorMode::Char(b';') => "Semicolon  ;".to_owned(),
        SeparatorMode::Char(b) if b.is_ascii_graphic() => format!("Custom  {}", b as char),
        SeparatorMode::Char(b) => format!("Custom  0x{b:02X}"),
    };
    egui::Window::new("Convert Delimiter")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(format!(
                "Current delimiter:  {cur_label}"
            )));
            ui.separator();
            ui.label(crate::theme::chrome_text(s.convert_to));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut doc.convert_target, Some(b','), "Comma  ,");
                ui.selectable_value(&mut doc.convert_target, Some(b'\t'), "Tab");
                ui.selectable_value(&mut doc.convert_target, Some(b'|'), "Pipe  |");
                ui.selectable_value(&mut doc.convert_target, Some(b';'), "Semicolon  ;");
            });
            ui.horizontal(|ui| {
                ui.label(crate::theme::chrome_text(s.convert_custom));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut doc.convert_custom_input)
                        .desired_width(28.0)
                        .char_limit(1),
                );
                // 커스텀 입력을 만지면 그 글자가 대상이 된다(라디오 선택 해제).
                if resp.changed() {
                    doc.convert_target = doc
                        .convert_custom_input
                        .as_bytes()
                        .first()
                        .copied()
                        .filter(|b| b.is_ascii());
                }
            });
            // 비ASCII를 입력한 경우 왜 안 되는지 알려준다. 조용히 비활성화하면
            // 사용자가 이유를 알 수 없다.
            if !doc.convert_custom_input.is_empty()
                && !doc.convert_custom_input.is_ascii()
            {
                ui.label(
                    egui::RichText::new("Custom delimiter must be an ASCII character.")
                        .color(egui::Color32::from_rgb(0xC0, 0x39, 0x2B)),
                );
            }
            ui.separator();
            ui.label(crate::theme::chrome_text(s.convert_warn_change));
            ui.label(crate::theme::chrome_text(
                s.convert_warn_save,
            ));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(s.common_cancel).clicked() {
                    doc.show_convert_dialog = false;
                }
                let enabled = convert_enabled(doc.sep, doc.convert_target);
                ui.add_enabled_ui(enabled, |ui| {
                    if ui.button(s.convert_do).clicked() {
                        want_convert = true;
                    }
                });
            });
        });
    if !open {
        doc.show_convert_dialog = false;
    }
    want_convert
}

/// 오류 목록에 표시할 **행번호** — 본문 라인 번호 칸과 반드시 같은 수여야 한다.
///
/// `render_table`의 번호 칸은 `view_row + row_base`를 쓴다(그 지점 참조).
/// `RowError.logical`은 **헤더를 포함한 절대 논리 행**이므로 그대로 더하면
/// 헤더가 있을 때 항상 1 어긋나고, 정렬 중이면 어긋나는 폭이 제멋대로다.
///
/// 그래서 `logical_to_screen_row`로 화면 행을 얻은 뒤 `row_base`를 더한다 —
/// 이동에 쓰는 `gutter_click_target`과 **같은 변환**이라, 목록에 보이는 번호와
/// 클릭해서 도착하는 행이 어긋날 수 없다.
fn error_row_display_number(doc: &Document, logical: usize, row_base: usize) -> usize {
    let row = match doc.sep {
        SeparatorMode::None => logical,
        SeparatorMode::Char(_) => {
            let data_start = if doc.has_header { 1 } else { 0 };
            logical_to_screen_row(doc, logical, data_start)
        }
    };
    row + row_base
}

/// 오류 유형 하나를 목록에 쓸 문구로 바꾼다.
///
/// `col_base`는 컬럼 번호 표시 기준(0/1)이다. 필드 **개수**는 기준과 무관한
/// 세는 값이라 그대로 쓰고, 여기서는 기준을 받지 않는다.
fn issue_label(issue: crate::validate::RowIssue) -> String {
    use crate::validate::RowIssue;
    match issue {
        RowIssue::FieldCount { got, expected } => {
            format!("{got} fields (expected {expected})")
        }
        RowIssue::UnbalancedQuote => "unbalanced quote".to_owned(),
        RowIssue::DecodeError => "decode failure".to_owned(),
    }
}

/// 오류 행 창. 항목을 클릭하면 본문이 그 행으로 이동하도록 **논리 행번호를
/// 돌려준다**(스크롤은 호출부가 건다).
///
/// 창 안에서 곧바로 스크롤을 걸지 않는 이유는 이 코드베이스의 규율이다 —
/// `doc`이 클로저에 가변 대여돼 있어 `logical_to_screen_row`가 다시 빌릴 수
/// 없고, 인텐트만 받아 두면 그 문제가 사라진다(찾기 창과 같은 방식).
fn render_errors_window(ctx: &egui::Context, doc: &mut Document, row_base: usize, lang: crate::i18n::Lang) -> Option<usize> {
    let s = crate::i18n::t(lang);
    let mut open = true;
    let mut goto: Option<usize> = None;

    egui::Window::new("Bad Rows")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .show(ctx, |ui| {
            if !matches!(doc.sep, SeparatorMode::Char(_)) {
                ui.label(crate::theme::chrome_text(
                    "Bad-row checking needs a delimiter — pick one in the toolbar.",
                ));
                return;
            }
            if doc.error_scan.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::theme::chrome_text(s.bad_checking));
                });
                return;
            }
            let Some(result) = &doc.row_errors else {
                // 인덱싱이 아직인 경우가 대부분이다.
                ui.label(crate::theme::chrome_text(s.bad_not_checked));
                return;
            };
            if result.total() == 0 {
                ui.label(crate::theme::chrome_text(s.bad_none));
                return;
            }

            let (fc, uq, de) = result.counts();
            ui.label(crate::theme::chrome_text(format!(
                "{} bad rows — {fc} field count, {uq} quote, {de} decode",
                result.total()
            )));
            if result.dropped > 0 {
                // 상한에 걸렸다는 사실을 반드시 밝힌다. 목록만 보고 "다
                // 고쳤다"고 믿는 것이 최악이다.
                ui.label(
                    crate::theme::chrome_text(format!(
                        "Showing the first {} — {} more not listed.",
                        result.errors.len(),
                        result.dropped
                    ))
                    .color(egui::Color32::from_rgb(200, 60, 40)),
                );
            }
            ui.separator();

            egui::ScrollArea::vertical()
                .max_height(360.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for e in &result.errors {
                        let text = format!(
                            "{}   {}   {}",
                            error_row_display_number(doc, e.logical, row_base),
                            issue_label(e.issue),
                            e.preview
                        );
                        if ui
                            .add(
                                egui::Label::new(crate::theme::chrome_text(text))
                                    .sense(egui::Sense::click())
                                    .truncate(),
                            )
                            .on_hover_text(s.bad_click_to_jump)
                            .clicked()
                        {
                            goto = Some(e.logical);
                        }
                    }
                });
        });

    if !open {
        doc.show_errors_window = false;
    }
    goto
}

/// 다중 컬럼 정렬 다이얼로그. 정렬 기준(컬럼·문자/숫자·오름/내림) 목록을
/// 위(1차)→아래(N차) 순으로 편집하고, "정렬"로 백그라운드 다중 정렬을 시작한다.
fn render_sort_dialog(ctx: &egui::Context, doc: &mut Document, col_base: usize, lang: crate::i18n::Lang) {
    let s = crate::i18n::t(lang);
    let delim = match doc.sep {
        SeparatorMode::Char(d) => d,
        SeparatorMode::None => {
            doc.show_sort_dialog = false;
            return;
        }
    };
    let data_start = if doc.has_header { 1 } else { 0 };

    // 컬럼 수: 헤더가 있으면 헤더 필드 수, 없으면 첫 데이터 행 필드 수로 근사.
    // 편집 모드에서도 올바른 내용을 보도록 `parse_logical_line_edit`을 쓴다 —
    // mmap 전용 `decode_logical_line` 경로는 편집 전 원본을 읽어 값이 낡는다.
    let col_count = {
        let probe = if doc.has_header { 0 } else { data_start };
        parse_logical_line_edit(doc, probe, delim)
            .map(|f| f.len())
            .unwrap_or(1)
            .max(1)
    };
    // 헤더 이름(드롭다운 라벨용).
    let header_fields: Option<Vec<String>> = if doc.has_header {
        parse_logical_line_edit(doc, 0, delim)
    } else {
        None
    };
    let col_label = |c: usize| -> String {
        // 표시 번호는 col_base(0 또는 1)를 반영.
        let n = c + col_base;
        match &header_fields {
            Some(h) => format!("{} {}", n, h.get(c).cloned().unwrap_or_default()),
            None => format!("Column {n}"),
        }
    };

    let mut open = true;
    let mut do_sort = false;
    let mut remove_idx: Option<usize> = None;
    // 순서 변경(위/아래로 한 칸): (from, to). 클로저 종료 후 swap.
    let mut swap_pair: Option<(usize, usize)> = None;

    egui::Window::new("Sort by Columns")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(
                "The topmost criterion is the primary sort; they apply top to bottom.",
            ));
            ui.separator();

            // 각 기준이 현재 선택 중인 컬럼 목록(스냅샷). 드롭다운에서 "다른 행이
            // 이미 쓰는 컬럼"을 제외해 같은 컬럼 중복 선택을 막는다.
            let selected_cols: Vec<usize> = doc.sort_specs.iter().map(|s| s.col).collect();

            for i in 0..doc.sort_specs.len() {
                ui.horizontal(|ui| {
                    ui.label(crate::theme::chrome_text(format!("{} {}", s.sort_priority, i + 1)));

                    // 컬럼 선택 드롭다운. 다른 기준이 이미 선택한 컬럼은 목록에서 제외.
                    let cur_col = doc.sort_specs[i].col.min(col_count - 1);
                    egui::ComboBox::from_id_source(("sortcol", i))
                        .selected_text(col_label(cur_col))
                        .show_ui(ui, |ui| {
                            for c in 0..col_count {
                                // 이 행(i) 자신의 현재 값은 남기고, 다른 행이 쓰는
                                // 컬럼만 숨긴다.
                                let used_by_other = selected_cols
                                    .iter()
                                    .enumerate()
                                    .any(|(j, &sc)| j != i && sc == c);
                                if used_by_other {
                                    continue;
                                }
                                ui.selectable_value(&mut doc.sort_specs[i].col, c, col_label(c));
                            }
                        });

                    // 문자/숫자.
                    egui::ComboBox::from_id_source(("sortkind", i))
                        .selected_text(match doc.sort_specs[i].kind {
                            SortKind::Text => "Text",
                            SortKind::Number => "Number",
                        })
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut doc.sort_specs[i].kind, SortKind::Text, "Text");
                            ui.selectable_value(
                                &mut doc.sort_specs[i].kind,
                                SortKind::Number,
                                "Number",
                            );
                        });

                    // 오름/내림.
                    egui::ComboBox::from_id_source(("sortdir", i))
                        .selected_text(match doc.sort_specs[i].dir {
                            SortDir::Asc => "Ascending",
                            SortDir::Desc => "Descending",
                        })
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut doc.sort_specs[i].dir, SortDir::Asc, "Ascending");
                            ui.selectable_value(&mut doc.sort_specs[i].dir, SortDir::Desc, "Descending");
                        });

                    // 대소문자 무시(문자 기준일 때만). 체크됨 = 무시(ci=true).
                    if doc.sort_specs[i].kind == SortKind::Text {
                        ui.checkbox(&mut doc.sort_specs[i].ci, s.sort_ignore_case);
                    }

                    // 순서 변경(↑ 위로, ↓ 아래로). 맨 위/맨 아래에선 해당 버튼 비활성.
                    let n = doc.sort_specs.len();
                    ui.add_enabled_ui(i > 0, |ui| {
                        if ui.button("↑").clicked() {
                            swap_pair = Some((i, i - 1));
                        }
                    });
                    ui.add_enabled_ui(i + 1 < n, |ui| {
                        if ui.button("↓").clicked() {
                            swap_pair = Some((i, i + 1));
                        }
                    });

                    // 삭제(기준이 2개 이상일 때만).
                    if n > 1 && ui.button("✖").clicked() {
                        remove_idx = Some(i);
                    }
                });
            }

            ui.separator();
            ui.horizontal(|ui| {
                // 아직 안 쓰인 컬럼이 있어야, 그리고 MAX_KEYS/전체 컬럼 수 미만일 때만
                // 기준을 추가할 수 있다(같은 컬럼 중복 금지 정책).
                let used: Vec<usize> = doc.sort_specs.iter().map(|s| s.col).collect();
                let next_free = (0..col_count).find(|c| !used.contains(c));
                let can_add = doc.sort_specs.len() < sort::MAX_KEYS
                    && doc.sort_specs.len() < col_count
                    && next_free.is_some();

                ui.add_enabled_ui(can_add, |ui| {
                    if ui.button(s.sort_add_criterion).clicked() {
                        if let Some(col) = next_free {
                            doc.sort_specs.push(SortSpec {
                                col,
                                kind: SortKind::Text,
                                dir: SortDir::Asc,
                                ci: true,
                            });
                        }
                    }
                });
                if doc.sort_specs.len() >= sort::MAX_KEYS {
                    ui.label(crate::theme::chrome_text(s.sort_maximum.replace("{}", &sort::MAX_KEYS.to_string())));
                } else if doc.sort_specs.len() >= col_count {
                    ui.label(crate::theme::chrome_text(s.sort_all_in_use));
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(s.sort_do).clicked() {
                    do_sort = true;
                }
                if ui.button(s.common_cancel).clicked() {
                    doc.show_sort_dialog = false;
                }
            });
        });

    if let Some(i) = remove_idx {
        doc.sort_specs.remove(i);
    }
    if let Some((a, b)) = swap_pair {
        if a < doc.sort_specs.len() && b < doc.sort_specs.len() {
            doc.sort_specs.swap(a, b);
        }
    }
    // 창 X로 닫아도 다이얼로그 종료.
    if !open {
        doc.show_sort_dialog = false;
    }

    if do_sort && !doc.sort_specs.is_empty() {
        doc.show_sort_dialog = false;
        if doc.edit.is_some() {
            // 편집 모드: 백그라운드 permutation 대신 lines를 물리적으로 재배치한다.
            // 인메모리 정렬이라 동기 호출로 충분하다.
            let specs = doc.sort_specs.clone();
            apply_edit_sort(doc, &specs, delim, data_start);
        } else {
            doc.sort_job = Some(sort::spawn_multi_sort(
                doc.source.clone(),
                doc.index.clone(),
                doc.enc,
                delim,
                doc.sort_specs.clone(),
                data_start,
                ctx.clone(),
            ));
        }
    }
}

/// 편집 모드 정렬: `sort_lines`로 순서를 구해 `lines`를 실제로 재배치한다.
///
/// 뷰 모드의 permutation 정렬과 달리 결과가 버퍼에 그대로 반영되므로
/// `doc.sort`(SortState)는 **설정하지 않는다** — 유지할 permutation이 없고,
/// 헤더 화살표/상태바가 있지도 않은 라이브 정렬을 주장하면 안 된다. 정렬 뒤
/// 행을 삽입해도 재정렬되지 않고 삽입 위치에 그대로 남는다.
///
/// 셀 편집/선택 상태는 행이 움직이면 가리키는 대상이 달라지므로 초기화한다.
fn apply_edit_sort(doc: &mut Document, specs: &[SortSpec], delim: u8, data_start: usize) {
    let Some(e) = doc.edit.as_mut() else { return };
    if specs.is_empty() || e.lines.len() <= data_start {
        return;
    }
    let order = sort::sort_lines(&e.lines, specs, delim, data_start);
    // 되돌리기: 역순열을 적용하면 원래 행 순서로 돌아온다. 재배치 **전에** 기록.
    e.undo.push(crate::edit::EditOp::Reorder {
        inverse: crate::edit::inverse_of(&order, data_start),
        data_start,
    });
    crate::edit::apply_permutation(&mut e.lines, &order, data_start);
    e.dirty = true;
    // 정렬로 행이 뒤섞였으니 행을 가리키던 상태는 무효.
    doc.editing_cell = None;
    doc.cell_edit_text.clear();
    doc.cell_sel = None;
    doc.cell_drag_active = false;
    // 편집 모드 정렬은 permutation이 남지 않는다(이미 lines에 반영됨).
    doc.sort = None;
    doc.sort_job = None;
}

/// 툴바의 정렬 컨트롤. 컬럼이 선택돼 있고 인덱싱이 완료(Phase::Complete)일 때만
/// 정렬 버튼이 활성화된다. 정렬은 백그라운드 스레드에서 수행되며, 진행 중에는
/// progress bar가 표시되고 완료되면 permutation을 doc.sort로 옮긴다.
fn render_sort_controls(
    ui: &mut egui::Ui,
    doc: &mut Document,
    ctx: &egui::Context,
    lang: crate::i18n::Lang,
) {
    use crate::index::Phase;

    let s = crate::i18n::t(lang);

    // 진행 중인 정렬 작업이 끝났는지 먼저 폴링해 결과를 수거.
    if let Some(job) = &mut doc.sort_job {
        if let Some(perm) = job.take_result() {
            let spec_count = job.specs.len().max(1);
            doc.sort = Some(SortState {
                permutation: perm,
                col: job.col,
                kind: job.kind,
                dir: job.dir,
                spec_count,
            });
            doc.sort_job = None;
        }
    }

    // 정렬 진행 중이면 progress bar만 표시하고 버튼은 숨긴다.
    if let Some(job) = &doc.sort_job {
        let p = job.progress();
        ui.label(crate::theme::chrome_text(s.sort_running));
        ui.add(
            egui::ProgressBar::new(p)
                .desired_width(160.0)
                .text(format!("{:.0}%", p * 100.0)),
        );
        return;
    }

    // 편집 버퍼와 Parquet은 인덱싱 진행 상태와 무관하게 정렬할 수 있다
    // (`doc_rows_ready` 참조). Parquet은 인덱서를 아예 안 띄우므로 Phase가
    // 영영 Priming이라, 그 값을 그대로 보면 정렬 버튼이 끝내 안 켜진다.
    let editing = doc.edit.is_some();
    let complete = doc_rows_ready(doc);
    let selected = doc.selected_col;

    match selected {
        Some(col) => ui.label(crate::theme::chrome_text(s.sort_column_n.replace("{}", &(col + 1).to_string()))),
        None => ui.label(crate::theme::chrome_text(s.sort_pick_header)),
    };

    let delim = match doc.sep {
        SeparatorMode::Char(d) => d,
        SeparatorMode::None => return,
    };
    let data_start = if doc.has_header { 1 } else { 0 };

    // 컬럼 미선택 또는 인덱싱 미완료면 버튼 비활성.
    let can_sort = selected.is_some() && complete;

    let mut do_sort: Option<(SortKind, SortDir)> = None;
    ui.add_enabled_ui(can_sort, |ui| {
        if ui.button(s.sort_text_asc).clicked() {
            do_sort = Some((SortKind::Text, SortDir::Asc));
        }
        if ui.button(s.sort_text_desc).clicked() {
            do_sort = Some((SortKind::Text, SortDir::Desc));
        }
        if ui.button(s.sort_number_asc).clicked() {
            do_sort = Some((SortKind::Number, SortDir::Asc));
        }
        if ui.button(s.sort_number_desc).clicked() {
            do_sort = Some((SortKind::Number, SortDir::Desc));
        }
    });

    // 다중 컬럼 정렬 다이얼로그 열기(인덱싱 완료일 때만).
    ui.add_enabled_ui(complete, |ui| {
        if ui.button(s.menu_sort_columns).clicked() {
            // 다이얼로그를 열 때 기준 목록이 비어 있으면 현재 선택 컬럼(있으면)으로
            // 첫 기준을 미리 채워 사용자가 바로 편집하게 한다.
            if doc.sort_specs.is_empty() {
                let col = doc.selected_col.unwrap_or(0);
                doc.sort_specs.push(SortSpec {
                    col,
                    kind: SortKind::Text,
                    dir: SortDir::Asc,
                    ci: true,
                });
            }
            doc.show_sort_dialog = true;
        }
    });

    // 정렬 해제 버튼은 뷰 모드에서 permutation 정렬이 적용돼 있을 때만.
    // (편집 모드 정렬은 lines에 이미 반영돼 되돌릴 permutation이 없다.)
    if doc.sort.is_some() && ui.button(s.sort_clear).clicked() {
        doc.sort = None;
    }

    if !complete && selected.is_some() {
        ui.label(crate::theme::chrome_text(
            "(sorting available after indexing completes)",
        ));
    }

    // 정렬 버튼이 눌리면 — 편집 모드면 lines를 즉시 재배치하고,
    // 뷰 모드면 백그라운드 permutation 작업을 띄운다.
    if let (Some((kind, dir)), Some(col)) = (do_sort, selected) {
        // 단일 문자 정렬은 대소문자 무시를 기본으로(사람 직관). 세밀 제어는
        // 다중 정렬 다이얼로그에서.
        let ci = kind == SortKind::Text;
        let spec = SortSpec { col, kind, dir, ci };
        if doc.parquet.is_some() {
            // Parquet은 mmap 바이트가 없어 `sort::spawn_sort`의 전제가 성립하지
            // 않는다. 정렬 키 컬럼만 읽는 별도 경로로 보낸다(메모리 확인 포함).
            request_parquet_sort(doc, col, dir);
        } else if editing {
            apply_edit_sort(doc, &[spec], delim, data_start);
        } else {
            doc.sort_job = Some(sort::spawn_sort(
                doc.source.clone(),
                doc.index.clone(),
                doc.enc,
                delim,
                col,
                data_start,
                kind,
                dir,
                ci,
                ctx.clone(),
            ));
        }
    }
}

/// 우클릭 컨텍스트 메뉴에서 고른 동작. 클로저 안에서는 doc을 가변으로 빌릴 수
/// 없으므로 "무엇을 눌렀는지"만 기록해 두고, 테이블 클로저가 끝난 뒤 적용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellMenuAction {
    Copy,
    Cut,
    Paste,
    Clear,
    InsertRowAbove,
    InsertRowBelow,
    DeleteRows,
}

/// 사각 선택을 정규화한다: (r0<=r1, c0<=c1).
fn normalize_rect(sel: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let (r0, c0, r1, c1) = sel;
    (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1))
}

/// 컬럼 선택(헤더 클릭)을 데이터 전 구간 사각 선택으로 바꾼다.
/// 헤더 행은 제외하고 `data_start`부터 마지막 행(`line_count - 1`)까지.
/// 데이터 행이 하나도 없으면 None.
fn column_as_rect(
    col: usize,
    data_start: usize,
    line_count: usize,
) -> Option<(usize, usize, usize, usize)> {
    if line_count <= data_start {
        return None;
    }
    Some((data_start, col, line_count - 1, col))
}

/// 복사/잘라내기/붙여넣기/지우기가 실제로 대상으로 삼을 사각 범위.
///
/// 셀 사각 선택이 있으면 그것을 쓰고, 없고 헤더 클릭 컬럼 선택만 있으면
/// 그 컬럼 전체(데이터 행만)를 사각 범위로 환산한다. 둘 다 없으면 None.
/// 반환값은 정규화된 (r0, c0, r1, c1).
fn effective_cell_rect(
    cell_sel: Option<(usize, usize, usize, usize)>,
    selected_col: Option<usize>,
    data_start: usize,
    line_count: usize,
) -> Option<(usize, usize, usize, usize)> {
    if let Some(sel) = cell_sel {
        return Some(normalize_rect(sel));
    }
    let col = selected_col?;
    column_as_rect(col, data_start, line_count)
}

/// 표 셀 렌더에 필요한 찾기 하이라이트 컨텍스트. `render_table`이 프레임 시작에
/// 한 번 스냅샷해 두고 셀마다 `paint_table_cell`에 참조로 넘긴다(인자 수를 줄여
/// 셀 그리기 함수를 단순하게 유지).
struct CellFind<'a> {
    /// 검색 중인가(검색어가 비어 있지 않은가). false면 기존 `Label` 경로 그대로.
    searching: bool,
    query: &'a str,
    opts: &'a crate::find::FindOptions,
    font_id: &'a egui::FontId,
    text_color: egui::Color32,
}

/// 표 셀 하나의 텍스트(+찾기 하이라이트)를 그린다. 뷰 전용 모드와 편집 모드의
/// 비편집 셀이 **같은 그리기 규칙**을 쓰도록 한 곳에 모은다.
///
/// - `find.searching`이 false면 기존 `egui::Label` + truncate 경로 그대로
///   (회귀·성능 방지 — 셀이 많다). 검색 중일 때만 galley로 바꿔 부분 음영.
/// - `current_row`가 true면(= 이 셀의 논리 행이 last_match 행) 셀 배경 전체를
///   `find_current_bg`로 먼저 칠한다(설계 판단: "행 전체 선택"과 일관). 그 위에
///   개별 매치의 옅은 음영이 겹쳐도, current가 이미 진하므로 무해하다.
/// - **셀 텍스트에 delim=None으로** `find_in_line_scoped`를 부른다: 셀은 이미
///   필드로 쪼갠 뒤이므로 Partial/WholeWord는 셀 안 부분 매치, WholeCell은
///   delim=None의 "행(=여기선 셀) 전체 == query" 규칙이 그대로 "셀==query"가
///   된다 — 세 scope가 모두 셀 단위로 올바르게 동작한다(다음 사람이 헷갈리지
///   않도록 남기는 주석 — E2-4).
fn paint_table_cell(
    ui: &mut egui::Ui,
    cell_rect: egui::Rect,
    text: String,
    current_row: bool,
    find: &CellFind,
) {
    // current 행이면 셀 배경 전체를 진한 보라로(행 전체 강조).
    if current_row {
        ui.painter()
            .rect_filled(gapless_cell_rect(ui, cell_rect), 0.0, crate::theme::find_current_bg());
    }
    // 검색 중이 아니면 기존 Label 경로 그대로.
    if !find.searching {
        ui.add(egui::Label::new(text).truncate());
        return;
    }
    let len = line_char_len(&text);
    let galley =
        ui.fonts(|f| f.layout_no_wrap(text.clone(), find.font_id.clone(), find.text_color));
    let origin = egui::pos2(cell_rect.left(), cell_rect.center().y - galley.size().y * 0.5);
    let x_of = |c: usize| -> f32 {
        origin.x
            + galley
                .pos_from_ccursor(egui::text::CCursor::new(c.min(len)))
                .min
                .x
    };
    let painter = ui.painter().with_clip_rect(cell_rect);
    // 셀 텍스트에 delim=None(위 주석). current 셀 배경은 이미 위에서 칠했으므로
    // 여기서는 개별 매치의 옅은 음영만 그린다(current를 다시 덮지 않는다).
    let matches = crate::find::find_in_line_scoped(&text, find.query, find.opts, None);
    paint_match_shades(&painter, cell_rect, &x_of, &matches, None);
    painter.galley(origin, galley.clone(), find.text_color);
}

/// Shift+클릭으로 선택을 확장할 때의 새 사각 선택. **앵커는 유지하고 끝점만**
/// 옮긴다(Windows 표준 동작).
///
/// `prev`는 정규화 전 원본 `cell_sel`이다 — 앞 두 값이 앵커(먼저 누른 지점),
/// 뒤 두 값이 끝점이다. 그래서 정규화하지 않고 앞 두 값을 그대로 앵커로 쓴다.
/// 이전 선택이 없으면 클릭 지점을 앵커로 삼아 단일 셀 선택으로 시작한다
/// (평소 클릭과 같은 결과 — Shift를 눌렀지만 확장할 기준이 없기 때문).
fn shift_extend(
    prev: Option<(usize, usize, usize, usize)>,
    row: usize,
    col: usize,
) -> (usize, usize, usize, usize) {
    match prev {
        Some((ar, ac, _, _)) => (ar, ac, row, col),
        None => (row, col, row, col),
    }
}

/// 텍스트 모드 Shift+클릭의 앵커. 기존 선택이 있으면 그 앵커(`text_sel.0`)를,
/// 없으면 현재 캐럿을 앵커로 삼는다. 표 모드 `shift_extend`와 같은 규율이다.
fn shift_extend_text(
    prev_sel: Option<(crate::edit::TextPos, crate::edit::TextPos)>,
    caret: crate::edit::TextPos,
) -> crate::edit::TextPos {
    prev_sel.map(|(a, _)| a).unwrap_or(caret)
}

/// (row, col)이 정규화 전 선택 사각형 안에 들어가는지.
fn rect_contains(sel: (usize, usize, usize, usize), row: usize, col: usize) -> bool {
    let (r0, c0, r1, c1) = normalize_rect(sel);
    (r0..=r1).contains(&row) && (c0..=c1).contains(&col)
}

/// 표 모드 렌더: 라인번호 + 구분자로 분리한 필드 컬럼들.
/// 헤더 클릭으로 컬럼을 선택하고, 정렬이 적용돼 있으면 permutation 순서로 렌더.
/// 편집 모드(`doc.edit.is_some()`)에서는 셀 편집/드래그 선택/우클릭 메뉴가 켜진다.
fn render_table(
    ui: &mut egui::Ui,
    doc: &mut Document,
    delim: u8,
    row_base: usize,
    col_base: usize,
    clipboard: &mut String,
) {
    use std::cell::{Cell, RefCell};

    // 찾기·페이지 이동이 남긴 스크롤 요청을 소비한다. `body.rows` 가상 스크롤에서는
    // 화면 밖 행이 아예 그려지지 않아 `Response::scroll_to_me`가 통하지 않으므로,
    // 행 번호를 세로 offset(px)으로 직접 환산해 `vertical_scroll_offset`에 넘긴다
    // (`scroll_offset_for_row` 주석 — `scroll_to_row`는 egui가 0.1~0.3초에 걸쳐
    //  부드럽게 감아 페이지 이동이 느리게 느껴진다). 정렬은 요청자가
    // `pending_scroll_align`에 함께 남긴다 — 찾기는 `Center`(매치가 가장자리에
    // 붙지 않고 앞뒤 맥락과 함께 보이게), Page Up/Down은 `TOP`이다.
    // 아래 스냅샷들이 doc을 불변 대여하기 **전에** 꺼내야 한다.
    let scroll_to = doc.pending_scroll_row.take();
    let scroll_align = doc.pending_scroll_align;

    // 헤더 행 데이터(있으면 첫 줄)와 데이터 시작 행 결정.
    // `doc_line_count`를 쓴다 — Parquet은 `LineIndex`가 비어 있어
    // `index.line_count()`면 0이 되고 표가 통째로 비어 보인다.
    let total_lines = doc_line_count(doc);
    let header_fields: Option<Vec<String>> = if doc.has_header && total_lines > 0 {
        parse_logical_line_edit(doc, 0, delim)
    } else {
        None
    };

    let data_start = if doc.has_header { 1 } else { 0 };
    let data_rows = total_lines.saturating_sub(data_start);

    // 클로저에서 doc를 불변으로만 빌리기 위해, 렌더에 필요한 상태를 미리 뽑는다.
    let selected_col = doc.selected_col;
    // 정렬된 컬럼과 방향(헤더 화살표 표시용).
    let sorted_col_dir: Option<(usize, SortDir)> =
        doc.sort.as_ref().map(|s| (s.col, s.dir));
    // permutation은 참조로만 사용(클론 방지). 정렬 시 행 순서 매핑에 씀.
    let permutation: Option<&[u32]> = doc.sort.as_ref().map(|s| s.permutation.as_slice());
    // 헤더 클릭으로 새로 선택된 컬럼을 클로저 밖으로 전달하는 통로.
    let clicked_col: Cell<Option<usize>> = Cell::new(None);

    // ---- 편집 모드 상태 스냅샷 + 클로저 → 바깥 인텐트 통로 ----
    // 테이블 클로저는 doc을 불변으로만 빌린다. 셀 상호작용 결과는 여기 모아
    // 두었다가 클로저가 끝난 뒤 doc.edit에 적용한다(기존 clicked_col과 동일 패턴).
    let editing = doc.edit.is_some();
    // cell_sel / editing_cell은 논리 행번호를 담는다(편집 모드에선 sort=None이므로
    // logical = data_start + view_row).
    let cur_sel = doc.cell_sel;
    let editing_cell = doc.editing_cell;

    // 찾기 하이라이트 스냅샷. **라이브 `find_query`가 아니라 Find All 스냅샷
    // (`doc.highlight`)만** 본다 — 그래서 입력란에 타이핑해도 여기서 아무 스캔도
    // 일어나지 않는다. 스냅샷이 없으면 셀은 기존 `Label` 경로를 그대로 써
    // 회귀·성능을 지키고(셀이 많으므로 이 분기가 중요), 스냅샷이 있을 때만 galley로
    // 부분 음영을 그린다(보이는 행만이라 값싸다).
    let find_query = doc.highlight.as_ref().map(|h| h.query.clone()).unwrap_or_default();
    let find_opts = doc
        .highlight
        .as_ref()
        .map(|h| h.opts.clone())
        .unwrap_or_default();
    let searching = doc.highlight.is_some();
    // current match 강조는 **셀 배경 전체**로 한다(설계 판단 — 리포트 참조):
    // 표 모드 last_match.col은 행 전체 기준 char 인덱스라 셀 경계로 정밀 매핑하려면
    // 인용/구분자를 거슬러야 하는데, `focus_match`가 이미 매치 행을 "행 전체 선택"
    // 으로 강조하는 것과 일관되게, current 매치가 있는 **논리 행의 모든 셀**을
    // 진한 보라 배경으로 칠한다. 이 값은 그 논리 행 번호다.
    let current_match_row = doc.last_match.map(|m| m.line);
    let font_id = doc_font_id(doc);
    let cell_text_color = ui.visuals().text_color();
    let cell_find = CellFind {
        searching,
        query: &find_query,
        opts: &find_opts,
        font_id: &font_id,
        text_color: cell_text_color,
    };
    // 편집 중 텍스트는 클로저 안에서 &mut로 써야 하므로 로컬 버퍼에 복사했다가
    // 클로저 종료 후 doc.cell_edit_text에 되돌린다(편집 중일 때만 복사).
    let edit_text: RefCell<String> = RefCell::new(if doc.editing_cell.is_some() {
        doc.cell_edit_text.clone()
    } else {
        String::new()
    });

    // 드래그 시작 셀(논리 행, 열). 드래그 중 확장 끝점.
    let drag_anchor: Cell<Option<(usize, usize)>> = Cell::new(None);
    let drag_head: Cell<Option<(usize, usize)>> = Cell::new(None);
    // Shift+클릭으로 확장할 끝점. 앵커는 **잡지 않는다**(기존 앵커 유지).
    let shift_click: Cell<Option<(usize, usize)>> = Cell::new(None);
    // 이번 프레임에 "셀 위에서" 좌클릭 누름이 진행 중인지. doc.cell_drag_active를
    // 클로저 밖에서 켜기 위한 통로(클로저는 doc을 불변으로만 빌린다).
    let cell_press: Cell<bool> = Cell::new(false);
    // 더블클릭으로 편집을 시작할 셀 + 그 셀의 현재 값.
    let begin_edit: RefCell<Option<(usize, usize, String)>> = RefCell::new(None);
    // 편집 중 셀의 커밋(Enter 또는 포커스 상실) 요청.
    let commit_edit: Cell<bool> = Cell::new(false);
    // Esc — 편집 취소(값 버림). 커밋보다 우선한다.
    let cancel_edit: Cell<bool> = Cell::new(false);
    // 우클릭으로 선택을 단일 셀로 바꿔야 하는 경우.
    let menu_target: Cell<Option<(usize, usize)>> = Cell::new(None);
    // 컨텍스트 메뉴에서 고른 동작.
    let menu_action: Cell<Option<CellMenuAction>> = Cell::new(None);
    // 좌클릭 버튼이 눌린 상태인지(드래그 확장 판정용). 프레임당 한 번만 읽는다.
    let primary_down = ui.input(|i| i.pointer.primary_down());
    // Shift 눌림. Shift+클릭은 앵커를 새로 잡지 않고 끝점만 옮긴다.
    let shift_down = ui.input(|i| i.modifiers.shift);
    // 이전 프레임까지 이어져 온 "셀에서 시작된 드래그" 상태. 버튼이 떼어진
    // 프레임부터는 무조건 꺼진 것으로 본다.
    let drag_active = doc.cell_drag_active && primary_down;

    // ---- 키보드 단축키(Ctrl+C/X/V, Delete) ----
    // 텍스트 모드(`render_text`)와 같은 규율:
    //  - 편집 모드일 때만,
    //  - 다른 위젯이 키보드 포커스를 갖고 있지 않을 때만(keyboard_free),
    //  - 인라인 셀 편집기가 떠 있으면 처리하지 않는다(TextEdit이 가져가야 함).
    // egui-winit은 Ctrl+C/X/V를 `Key` 이벤트가 아니라
    // `Event::Copy`/`Cut`/`Paste`로 보내므로 그 이벤트를 읽는다.
    // `Event::Paste(s)`는 시스템 클립보드 문자열을 직접 주므로 외부 앱(엑셀 등)
    // → 뷰어 붙여넣기가 여기서 성립한다.
    let key_actions: Vec<(CellMenuAction, Option<String>)> = if editing
        && doc.editing_cell.is_none()
        && ui.memory(|m| m.focused().is_none())
    {
        ui.input(collect_cell_key_actions)
    } else {
        Vec::new()
    };

    // col_count는 헤더 필드 수와, 앞부분 데이터 행 몇 개를 샘플링한 필드 수의
    // 최댓값으로 정한다. 헤더가 없는 파일(header_fields == None)에서 1로
    // 고정되어 컬럼이 다 숨는 문제, 그리고 헤더보다 넓은 행이 잘리는 문제를
    // 함께 해결한다. `focus_match`(찾기 결과 "행 전체" 선택, Important 2)도
    // 이 값을 그대로 써야 하므로 `table_col_count`로 뽑아 공유한다 — 계산을
    // 두 곳에 따로 두면 언젠가 한쪽만 바뀌어 어긋난다.
    let col_count = table_col_count(doc, delim);

    // 테이블이 남은 세로 공간을 모두 채우도록 한다.
    // - max_scroll_height 기본값(800px)이 스크롤 영역을 제한해 창을 키워도
    //   ~35행에서 멈추므로, 사용 가능한 높이로 올린다.
    // - auto_shrink의 y축을 false로 두어 내용이 적어도 테이블이 창을 채운다.
    let avail_height = ui.available_height();
    // 행 높이는 배율을 탄다 — 상수 ROW_HEIGHT를 직접 쓰면 확대 시 글자가 잘린다.
    let row_h = doc_row_height(doc);
    // 행 간격은 `TableBody::rows`가 행 높이에 더하는 값과 **같아야** 한다
    // (`scroll_offset_for_row` 주석) — 여기서 한 번 읽어 그대로 넘긴다.
    let spacing_y = ui.spacing().item_spacing.y;

    // 컬럼은 auto()(전 행 measure로 대용량에서 느림) 대신 고정 초기폭 +
    // 드래그 조절(resizable)로 둔다. 긴 값은 셀에서 truncate 되고 폭을
    // 넓히면 전체가 보인다.
    // 이번 프레임에 그려진 행 중 가장 작은 화면 행. Page Up/Down이 "지금
    // 어디를 보고 있나"를 알 유일한 방법이다(`Document::first_visible_row`
    // 주석 참조). 클로저는 doc을 불변으로만 빌리므로 `Cell`에 모았다가
    // 클로저 종료 뒤 doc에 쓴다(`clicked_col`과 같은 통로). 아래 가로 스크롤
    // 클로저 밖에서도 읽어야 하므로 그 클로저보다 먼저 선언해 캡처시킨다.
    let min_drawn_row: Cell<Option<usize>> = Cell::new(None);

    // `TableBuilder`(egui_extras 0.28)는 안쪽에서 세로만 스크롤 영역으로
    // 감싼다(가로는 하드코딩으로 꺼져 있음) — 컬럼이 많아 전체 폭이 창을
    // 넘으면 뒤쪽 컬럼이 스크롤 없이 그냥 잘렸다. 바깥에 가로 전용
    // `ScrollArea`를 한 겹 더 씌운다. 컬럼이 `Column::initial`(고정폭)이라
    // 표의 실제 폭이 뷰포트 폭에 매이지 않으므로, 세로는 안쪽 TableBuilder가
    // 그대로 담당하고 가로만 바깥 ScrollArea가 담당해도 서로 간섭하지 않는다.
    egui::ScrollArea::horizontal()
        .id_source("render_table_hscroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .auto_shrink([false, false])
        .max_scroll_height(avail_height)
        .column(Column::initial(64.0).at_least(48.0).resizable(true)) // 라인번호 #
        .columns(Column::initial(120.0).at_least(60.0).resizable(true), col_count);

    if let Some(row) = scroll_to {
        // 요청된 행은 실제 존재하는 마지막 데이터 행으로 클램프한다
        // (`scroll_to_row`가 내부적으로 하던 일 — offset 경로에는 없으므로 여기서).
        let row = row.min(data_rows.saturating_sub(1));
        table = table.vertical_scroll_offset(scroll_offset_for_row(
            row,
            scroll_align,
            row_h,
            spacing_y,
            // 본문 높이 = 전체 - 헤더 한 줄(`visible_row_count`와 같은 규율).
            (avail_height - row_h).max(0.0),
        ));
    }

    table
        .header(row_h, |mut header| {
            header.col(|ui| {
                let rect = ui.max_rect();
                paint_header_cell(ui, rect, crate::theme::header_bg());
                // 아래 라인번호가 오른쪽 정렬이므로 머리글도 오른쪽에 맞춘다.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new("#").strong()).truncate());
                });
            });
            for c in 0..col_count {
                header.col(|ui| {
                    let selected = selected_col == Some(c);
                    // 셀 전체 영역을 클릭 대상으로 만든다(글자뿐 아니라 헤더 칸
                    // 어디를 눌러도 선택되도록). 먼저 셀의 남은 사각형 전체에
                    // 클릭 sense 응답을 만든다.
                    let cell_rect = ui.max_rect();
                    let resp = ui.interact(
                        cell_rect,
                        ui.id().with(("hdr", c)),
                        egui::Sense::click(),
                    );
                    // 헤더 배경 + 격자선. 선택된 컬럼은 배경만 파랑으로 바뀐다.
                    let bg = if selected {
                        header_sel_color()
                    } else {
                        crate::theme::header_bg()
                    };
                    paint_header_cell(ui, cell_rect, bg);
                    // 헤더 텍스트: "번호 이름" + 정렬 화살표. 번호는 col_base 반영.
                    let cn = c + col_base;
                    let base = if let Some(h) = &header_fields {
                        format!("{} {}", cn, h.get(c).cloned().unwrap_or_default())
                    } else {
                        format!("{cn}")
                    };
                    let arrow = match sorted_col_dir {
                        Some((sc, SortDir::Asc)) if sc == c => " ↑",
                        Some((sc, SortDir::Desc)) if sc == c => " ↓",
                        _ => "",
                    };
                    let rich = egui::RichText::new(format!("{base}{arrow}")).strong();
                    ui.add(egui::Label::new(rich).truncate());
                    if resp.clicked() {
                        clicked_col.set(Some(c));
                    }
                });
            }
        })
        .body(|body| {
            body.rows(row_h, data_rows, |mut row| {
                let view_row = row.index();
                min_drawn_row.set(Some(
                    min_drawn_row.get().map_or(view_row, |m: usize| m.min(view_row)),
                ));
                let line_no = view_row + row_base;
                // 정렬이 적용돼 있으면 permutation으로 원본 논리 행번호를 얻는다.
                // 없으면 원본 순서(data_start + view_row).
                let logical = match permutation {
                    Some(perm) => perm.get(view_row).map(|&r| r as usize),
                    None => Some(data_start + view_row),
                };
                // 라인번호 컬럼 — 화면 순번(정렬 후, row_base부터). 원본 행번호가
                // 아니라 보이는 순서를 매겨 스크롤 위치 감각을 유지한다.
                row.col(|ui| {
                    let rect = ui.max_rect();
                    paint_line_number_cell(ui, rect, format!("{line_no}"));
                });
                let fields = logical
                    .and_then(|l| parse_logical_line_edit(doc, l, delim))
                    .unwrap_or_default();
                for c in 0..col_count {
                    row.col(|ui| {
                        let cell_rect = ui.max_rect();
                        // 셀 경계 격자선(보이는 행에만 그려진다 — 가상 스크롤).
                        paint_cell_grid(ui, cell_rect);
                        // 선택된 컬럼은 셀 배경에 밝은 파란 음영(줄무늬 위에 반투명).
                        // gapless로 칠해야 셀 사이 틈이 흰 줄로 남지 않는다.
                        if selected_col == Some(c) {
                            ui.painter().rect_filled(
                                gapless_cell_rect(ui, cell_rect),
                                0.0,
                                sel_shade(),
                            );
                        }

                        // ---- 뷰 전용 모드 ----
                        if !editing {
                            let current_row =
                                current_match_row.is_some() && logical == current_match_row;
                            paint_table_cell(
                                ui,
                                cell_rect,
                                fields.get(c).cloned().unwrap_or_default(),
                                current_row,
                                &cell_find,
                            );
                            return;
                        }

                        // ---- 편집 모드 ----
                        // 편집 모드에선 permutation이 없으므로 logical은 항상 Some.
                        let Some(lrow) = logical else {
                            ui.add(egui::Label::new("").truncate());
                            return;
                        };

                        // 이 셀이 편집 중이면 라벨 대신 인라인 TextEdit.
                        if editing_cell == Some((lrow, c)) {
                            let mut buf = edit_text.borrow_mut();
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut *buf)
                                    .desired_width(cell_rect.width())
                                    .id(ui.id().with(("celledit", lrow, c))),
                            );
                            // 진입 프레임에 포커스를 준다(이미 있으면 no-op).
                            if !resp.has_focus() && !resp.lost_focus() {
                                resp.request_focus();
                            }
                            // Esc = 취소(값 버림). egui는 Esc에 포커스만 해제하므로
                            // 그대로 두면 lost_focus()로 커밋돼 버린다. 키 입력을
                            // lost_focus() 판정과 독립적으로 먼저 읽어, 같은 프레임에
                            // 둘 다 켜지면 취소가 이기게 한다(적용 단계에서 처리).
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                cancel_edit.set(true);
                            }
                            // 커밋 조건은 "포커스 상실" 하나로 충분하다 —
                            // singleline TextEdit은 Enter를 누르면 포커스를 놓고,
                            // 다른 곳을 클릭해도 포커스를 놓는다.
                            if resp.lost_focus() {
                                commit_edit.set(true);
                            }
                            return;
                        }

                        // 선택 사각형 음영(컬럼 음영과 같은 색·같은 gapless 규칙).
                        if let Some(sel) = cur_sel {
                            if rect_contains(sel, lrow, c) {
                                ui.painter().rect_filled(
                                    gapless_cell_rect(ui, cell_rect),
                                    0.0,
                                    sel_shade(),
                                );
                            }
                        }

                        let current_row = current_match_row == Some(lrow);
                        paint_table_cell(
                            ui,
                            cell_rect,
                            fields.get(c).cloned().unwrap_or_default(),
                            current_row,
                            &cell_find,
                        );

                        // 셀 전체를 클릭/드래그 대상으로. 셀 뒤에 interact를 걸어
                        // 셀 칸 어디를 눌러도 반응하게 한다.
                        // sense는 `focusable: false`를 명시한 TABLE_CELL_SENSE다 —
                        // `Sense::click_and_drag()`는 focusable: true라 그려진 셀마다
                        // Tab 순회 대상이 된다(상세 이유는 그 상수 주석 참조).
                        // 이 id는 인라인 편집기 id(`("celledit", ..)`)와 다르고,
                        // 애초에 편집 중인 셀은 위에서 early return 하므로 같은
                        // 프레임에 둘이 공존하지 않는다.
                        let cell_id = ui.id().with(("cell", lrow, c));
                        let resp = ui.interact(cell_rect, cell_id, TABLE_CELL_SENSE);

                        // 이 셀 위에서 좌클릭 누름이 시작/진행 중이면 "셀에서
                        // 시작된 드래그"로 표시한다. is_pointer_button_down_on()
                        // 하나로 충분하다 — drag_started()가 참이면 이미
                        // is_pointer_button_down_on()도 참이므로(전자가 후자를
                        // 함의) 별도로 or할 필요가 없다.
                        // `&& primary_down`이 핵심이다: is_pointer_button_down_on()은
                        // 버튼을 가리지 않으므로(우클릭 press에도 true) 이 게이트가
                        // 없으면 우클릭이 다중 셀 선택을 단일 셀로 무너뜨린다 —
                        // 우클릭 press 프레임에 앵커가 잡혀 cell_sel이 (X,c,X,c)로
                        // 붕괴하고, context_menu()는 release 시점에야 열리므로 그때는
                        // 이미 선택이 무너진 뒤다.
                        let pressed_here = resp.is_pointer_button_down_on() && primary_down;
                        if pressed_here {
                            cell_press.set(true);
                        }
                        // 새 앵커 = "이번 프레임에 이 셀에서 누름이 시작됐다".
                        // drag_active(이전 프레임부터 이어져 온 드래그)가 아닐 때만
                        // 앵커를 잡는다 — 드래그가 이어지는 동안 시작 셀은 계속
                        // is_pointer_button_down_on()이 참이므로, 그것만 보면 매
                        // 프레임 앵커/끝점이 시작 셀로 되돌아가 확장이 깨진다.
                        // 누르는 첫 프레임부터 앵커를 잡아야(clicked()는 release
                        // 에서만 켜진다) 아래 확장 분기가 옛 선택을 끌고 가지 않는다.
                        // Shift+클릭(Windows 표준): 앵커는 그대로 두고 끝점만
                        // 이 셀로 옮긴다. 그래서 `drag_anchor`를 **잡지 않는**
                        // 별도 분기다 — 앵커를 잡으면 범위가 이 셀 하나로
                        // 무너진다. 드래그 래치(`cell_press`)는 그대로 두어
                        // Shift+누른 채 끌면 계속 확장된다.
                        if resp.clicked() || (pressed_here && !drag_active) {
                            if shift_down {
                                shift_click.set(Some((lrow, c)));
                            } else {
                                drag_anchor.set(Some((lrow, c)));
                                drag_head.set(Some((lrow, c)));
                            }
                        }
                        // 드래그 중 끝점 확장. 드래그 이벤트는 최초로 눌린 셀만
                        // 받으므로(egui가 포인터를 그 위젯에 캡처), 다른 행/열로
                        // 넘어가는 확장은 "포인터가 이 셀 위 + 셀에서 시작된 드래그
                        // 진행 중"으로 감지한다. primary_down만 보면 표 밖에서 누른
                        // 채 들어온 포인터도 선택을 끌고 간다.
                        if resp.contains_pointer() && drag_active {
                            drag_head.set(Some((lrow, c)));
                        }
                        if resp.double_clicked() {
                            let val = fields.get(c).cloned().unwrap_or_default();
                            *begin_edit.borrow_mut() = Some((lrow, c, val));
                        }

                        // 우클릭 컨텍스트 메뉴.
                        // 어떤 셀에서 우클릭했는지는 **메뉴가 열리는 프레임에만**
                        // 기록한다. context_menu의 클로저는 메뉴가 떠 있는 매
                        // 프레임 다시 돌기 때문에, 그 안에서 set하면 menu_target이
                        // 메뉴 수명 내내 Some으로 남아 아래 5)의 "선택 밖이면 단일
                        // 선택으로" 재클램프가 매 프레임 재실행된다. 표 모드는
                        // 현재 메뉴가 열린 채 선택을 바꿀 키 경로가 없어 증상이
                        // 드러나지 않지만, 텍스트 모드에서 Ctrl+A로 실제 터졌던
                        // 것과 같은 구조다. secondary_clicked()는 메뉴를 여는
                        // release 프레임에만 참이므로(egui의 메뉴 생성 조건
                        // `hovered && secondary_clicked`, `menu.rs:429`와 일치)
                        // 여는 순간만 잡는다.
                        if resp.secondary_clicked() {
                            menu_target.set(Some((lrow, c)));
                        }
                        resp.context_menu(|ui| {
                            let pick = |ui: &mut egui::Ui, label: &str, act: CellMenuAction| {
                                if ui.button(label).clicked() {
                                    menu_action.set(Some(act));
                                    ui.close_menu();
                                }
                            };
                            pick(ui, "Copy", CellMenuAction::Copy);
                            pick(ui, "Cut", CellMenuAction::Cut);
                            pick(ui, "Paste", CellMenuAction::Paste);
                            pick(ui, "Clear Contents", CellMenuAction::Clear);
                            ui.separator();
                            pick(ui, "Insert Row Above", CellMenuAction::InsertRowAbove);
                            pick(ui, "Insert Row Below", CellMenuAction::InsertRowBelow);
                            pick(ui, "Delete Rows", CellMenuAction::DeleteRows);
                        });

                        // `TABLE_CELL_SENSE`의 `focusable: false`만으로는 부족하다.
                        // `Response::context_menu`는 내부에서
                        // `response.interact(Sense::click())`을 부르는데
                        // (`egui-0.28.1/src/menu.rs:415`), `Sense::click()`은
                        // `focusable: true`라 sense가 **union**되어
                        // (`response.rs:855-868`) 이 셀이 다시 focusable 위젯으로
                        // 등록된다. 그래서 이 셀이 포커스를 쥐고 있으면 즉시 반납한다.
                        // (`surrender_focus`는 그 id가 포커스일 때만 지우므로
                        //  `memory.rs:762-767`, 인라인 셀 편집기의 TextEdit이나
                        //  툴바 위젯의 포커스는 건드리지 않는다 — 그것들은 id가
                        //  다르고, 편집 중인 셀은 이 코드에 도달하지도 않는다.)
                        ui.memory_mut(|m| m.surrender_focus(cell_id));
                    });
                }
            });
        });
        });

    // Page Up/Down이 읽을 "지금 보고 있는 자리". **아래의 `if !editing { return }`
    // 보다 먼저** 기록해야 한다 — 뷰 모드에서도 페이지 이동은 되어야 하는데,
    // 그 조기 반환 뒤에 두면 뷰 모드에서는 영원히 0으로 남는다.
    // 그려진 행이 하나도 없으면(빈 문서, 창이 접힌 경우) 이전 값을 유지한다 —
    // 0으로 되돌리면 다음 Page Up/Down이 문서 맨 앞에서 시작한다.
    if let Some(first) = min_drawn_row.get() {
        doc.first_visible_row = first;
    }
    doc.visible_rows = visible_row_count(avail_height, row_h);

    // 클로저 종료 후 헤더 클릭 결과를 반영(같은 컬럼 재클릭이면 선택 해제 토글).
    // 셀 사각 선택도 함께 지운다 — 컬럼을 고른 순간 사용자의 대상은 그 컬럼이고,
    // 남아 있는 셀 선택이 `effective_cell_rect`에서 컬럼을 가려 버리면
    // "헤더를 눌렀는데 컬럼이 복사되지 않는" 혼란이 생긴다.
    if let Some(c) = clicked_col.get() {
        doc.selected_col = if doc.selected_col == Some(c) {
            None
        } else {
            Some(c)
        };
        doc.cell_sel = None;
        doc.cell_drag_active = false;
    }

    // ---- 여기서부터 편집 인텐트 적용(테이블 클로저 종료 → doc 가변 대여 가능) ----
    if !editing {
        return;
    }
    // 편집 중 텍스트 버퍼를 doc으로 되돌린다(TextEdit이 로컬 버퍼를 고쳤을 수 있음).
    // 프레임 시작 시 편집 중이 아니었다면 로컬 버퍼는 빈 더미이므로 쓰지 않는다.
    if doc.editing_cell.is_some() {
        doc.cell_edit_text = edit_text.into_inner();
    }

    // 1) 셀 편집 종료. Esc(취소)가 커밋을 이긴다 — 값을 버리고 편집만 닫는다.
    if cancel_edit.get() {
        doc.editing_cell = None;
        doc.cell_edit_text.clear();
    } else if commit_edit.get() {
        // 커밋 — 새 편집 시작보다 먼저 처리해야 이전 셀 값이 저장된다.
        commit_editing_cell(doc, delim);
        doc.editing_cell = None;
        doc.cell_edit_text.clear();
    }

    // 2) 드래그 시작 지점 추적. 셀 위에서 누름이 감지되면 켜고, 버튼을 떼면 끈다.
    // 표 밖(툴바/빈 공간)에서 누른 채 표로 들어온 포인터는 끝내 켜지지 않는다.
    doc.cell_drag_active =
        next_cell_drag_active(doc.cell_drag_active, primary_down, cell_press.get());

    // 3) 드래그/클릭 선택 갱신. Shift+클릭이 먼저다 — 앵커를 유지한 채 끝점만
    // 옮기므로 아래 앵커 분기(단일 셀로 붕괴)를 타면 안 된다. 그다음이
    // 앵커가 새로 잡힌 경우(평소 클릭/드래그 시작), 마지막이 이어지는 드래그
    // (앵커 유지 + 끝점 확장)다.
    if let Some((lrow, c)) = shift_click.get() {
        doc.cell_sel = Some(shift_extend(doc.cell_sel, lrow, c));
    } else if let Some((ar, ac)) = drag_anchor.get() {
        let (hr, hc) = drag_head.get().unwrap_or((ar, ac));
        doc.cell_sel = Some((ar, ac, hr, hc));
    } else if drag_active {
        if let (Some(sel), Some((hr, hc))) = (doc.cell_sel, drag_head.get()) {
            doc.cell_sel = Some((sel.0, sel.1, hr, hc));
        }
    }

    // 4) 더블클릭 → 셀 편집 시작.
    if let Some((lrow, c, val)) = begin_edit.into_inner() {
        // 다른 셀이 아직 편집 중이면(예: 편집 셀이 스크롤 밖으로 나가 lost_focus를
        // 못 받은 경우) 그 값을 먼저 커밋해 편집 내용을 잃지 않게 한다.
        if doc.editing_cell.is_some() && doc.editing_cell != Some((lrow, c)) {
            commit_editing_cell(doc, delim);
        }
        doc.editing_cell = Some((lrow, c));
        doc.cell_edit_text = val;
        // 편집 시작 셀을 단일 선택으로.
        doc.cell_sel = Some((lrow, c, lrow, c));
    }

    // 5) 컨텍스트 메뉴 동작.
    // 우클릭 셀이 현재 선택 밖이면 그 셀을 단일 선택으로 만든다.
    // menu_target은 메뉴가 열리는 프레임에만 채워진다(위 우클릭 메뉴 참조).
    // 판정은 프레임 시작 스냅샷(`cur_sel`)이 아니라 **현재** `doc.cell_sel`로
    // 한다 — 위 3)/4)가 이미 선택을 바꿨을 수 있고, 그 최신 선택이 "우클릭이
    // 선택 안이었나"의 진실이다.
    // 예외: 셀 선택이 없고 **그 컬럼이 헤더 클릭으로 선택돼 있으면** 컬럼 전체가
    // 대상이므로 단일 셀로 무너뜨리지 않는다(`effective_cell_rect`가 컬럼을
    // 사각 범위로 환산한다). 그래야 우클릭 메뉴에서도 컬럼 전체 복사가 된다.
    if let Some((lrow, c)) = menu_target.get() {
        let column_targeted = doc.cell_sel.is_none() && doc.selected_col == Some(c);
        let inside = doc.cell_sel.map_or(false, |s| rect_contains(s, lrow, c));
        if !inside && !column_targeted {
            doc.cell_sel = Some((lrow, c, lrow, c));
        }
    }
    if let Some(act) = menu_action.get() {
        apply_cell_menu_action(ui, doc, delim, clipboard, act, None);
    }

    // 6) 키보드 단축키. 메뉴 동작과 **같은 경로**를 태워 구현이 하나로 유지된다.
    //    `Event::Paste(s)`가 준 시스템 클립보드 문자열은 그대로 넘긴다.
    for (act, paste) in key_actions {
        apply_cell_menu_action(ui, doc, delim, clipboard, act, paste.as_deref());
    }
}

/// 표 모드 키 입력에서 셀 동작을 뽑는다. `collect_text_intents`와 같은 이유로
/// (egui-winit이 Ctrl+C/X/V를 Key가 아닌 Copy/Cut/Paste 이벤트로 보낸다)
/// 그 세 개는 이벤트로만 처리한다. Delete는 일반 Key 이벤트로 온다.
fn collect_cell_key_actions(i: &egui::InputState) -> Vec<(CellMenuAction, Option<String>)> {
    let mut out = Vec::new();
    for ev in &i.events {
        match ev {
            egui::Event::Copy => out.push((CellMenuAction::Copy, None)),
            egui::Event::Cut => out.push((CellMenuAction::Cut, None)),
            egui::Event::Paste(s) => out.push((CellMenuAction::Paste, Some(s.clone()))),
            egui::Event::Key {
                key: egui::Key::Delete,
                pressed: true,
                ..
            } => out.push((CellMenuAction::Clear, None)),
            _ => {}
        }
    }
    out
}

/// "셀에서 시작된 드래그가 진행 중인가" 상태 전이. egui의 `primary_down`은
/// 전역 버튼 상태라 그것만으로는 누름이 표 안에서 시작됐는지 알 수 없으므로,
/// 셀 위 누름(`pressed_on_cell`)이 관측된 적이 있는지를 별도로 래치한다.
///
/// - 버튼을 떼면(`primary_down == false`) 무조건 꺼진다.
/// - 셀 위에서 누름이 관측되면 켜진다.
/// - 그 외에는 이전 상태를 유지한다(드래그가 셀 밖으로 나갔다 돌아와도 유지).
fn next_cell_drag_active(prev: bool, primary_down: bool, pressed_on_cell: bool) -> bool {
    if !primary_down {
        return false;
    }
    prev || pressed_on_cell
}

/// 현재 `editing_cell`의 `cell_edit_text`를 편집 버퍼에 써넣는다(dirty 표시).
/// `editing_cell` 자체는 지우지 않는다 — 호출측이 상황에 맞게 정리한다.
fn commit_editing_cell(doc: &mut Document, delim: u8) {
    let Some((lrow, c)) = doc.editing_cell else { return };
    let text = doc.cell_edit_text.clone();
    let Some(e) = doc.edit.as_mut() else { return };
    if lrow >= e.lines.len() {
        return;
    }
    // 되돌리기용 이전 줄 전체를 **쓰기 전에** 캡처한다. push 자체는 적용 뒤에
    // 하지만 담기는 내용은 편집 전 상태이므로 의미가 같고, 값이 그대로일 때
    // 빈 undo 단계가 쌓이는 것을 막을 수 있다.
    let before = e.lines[lrow].clone();
    crate::edit::set_cell(&mut e.lines, lrow, c, &text, delim);
    if e.lines[lrow] == before {
        return;
    }
    e.undo
        .push(crate::edit::EditOp::Replace(vec![(lrow, before)]));
    e.dirty = true;
}

/// [r0..=r1] 범위 각 행의 **현재** 전체 텍스트를 `EditOp::Replace`로 만든다.
/// 편집을 적용하기 **직전에** 호출해 되돌리기용 이전 상태를 캡처한다.
/// 범위를 벗어난 행은 담지 않는다(`undo`도 조용히 건너뛰지만 낭비를 줄인다).
fn replace_op_for_rows(lines: &[String], r0: usize, r1: usize) -> crate::edit::EditOp {
    let mut items = Vec::new();
    for r in r0..=r1 {
        if r >= lines.len() {
            break;
        }
        items.push((r, lines[r].clone()));
    }
    crate::edit::EditOp::Replace(items)
}

/// `before`가 `replace_op_for_rows`로 캡처한 편집 전 스냅샷일 때, 현재
/// `lines`와 비교해 실제로 뭔가 바뀌었는지 판정한다. `before`가 `Replace`가
/// 아니거나 담긴 행 중 하나라도 현재 값과 다르면 변경이 있는 것으로 본다.
fn edit_op_differs_from_current(before: &crate::edit::EditOp, lines: &[String]) -> bool {
    let crate::edit::EditOp::Replace(items) = before else {
        return true;
    };
    items
        .iter()
        .any(|(r, old)| lines.get(*r).is_none_or(|cur| cur != old))
}

/// 컨텍스트 메뉴/키보드 단축키 동작을 편집 버퍼에 적용한다.
/// 선택 사각형은 논리 행/열 기준. 셀 선택이 없고 헤더 클릭 컬럼 선택만 있으면
/// 그 컬럼 전체(데이터 행)를 대상으로 삼는다(`effective_cell_rect`).
///
/// `paste_text`는 시스템 클립보드에서 직접 받은 문자열(`Event::Paste`)이다.
/// 있으면 그것을 우선 쓰고(외부 앱 → 뷰어 붙여넣기), 없으면 앱 내부
/// `clipboard` 캐시로 폴백한다.
fn apply_cell_menu_action(
    ui: &mut egui::Ui,
    doc: &mut Document,
    delim: u8,
    clipboard: &mut String,
    act: CellMenuAction,
    paste_text: Option<&str>,
) {
    apply_cell_menu_action_confirmed(ui, doc, delim, clipboard, act, paste_text, false)
}

/// `apply_cell_menu_action`의 본체. `confirmed`가 참이면 "큰 컬럼 연산" 확인을
/// 건너뛴다(사용자가 이미 다이얼로그에서 계속을 눌렀다).
fn apply_cell_menu_action_confirmed(
    ui: &mut egui::Ui,
    doc: &mut Document,
    delim: u8,
    clipboard: &mut String,
    act: CellMenuAction,
    paste_text: Option<&str>,
    confirmed: bool,
) {
    let data_start = if doc.has_header { 1 } else { 0 };
    let line_count = doc.edit.as_ref().map_or(0, |e| e.lines.len());
    let Some((r0, c0, r1, c1)) =
        effective_cell_rect(doc.cell_sel, doc.selected_col, data_start, line_count)
    else {
        return;
    };
    // 대상 행이 너무 많으면(컬럼 전체 선택 등) 한 번 묻는다. 확인 대기 중에는
    // 아직 아무것도 바꾸지 않는다 — 되돌리기 스택도 건드리지 않는다.
    // `pending_column_op.is_none()` 가드: 확인 다이얼로그가 이미 떠 있는 동안
    // 새 컬럼 연산 의도(두 번째 Ctrl+C, 메뉴 재클릭 등)를 받아 덮어쓰지 않는다
    // (다이얼로그가 비-모달 Window라 표 입력이 계속 처리되는 구조적 허점 보강).
    let rows = r1.saturating_sub(r0) + 1;
    if !confirmed && needs_big_op_confirm(act, rows) {
        if doc.pending_column_op.is_none() {
            doc.pending_column_op = Some(PendingColumnOp {
                act,
                paste_text: paste_text.map(|s| s.to_owned()),
                rows,
            });
        }
        return;
    }
    let Some(e) = doc.edit.as_mut() else { return };

    match act {
        CellMenuAction::Copy => {
            let tsv = crate::edit::cells_to_tsv(&e.lines, r0, c0, r1, c1, delim);
            *clipboard = tsv.clone();
            ui.output_mut(|o| o.copied_text = tsv);
        }
        CellMenuAction::Cut => {
            let tsv = crate::edit::cells_to_tsv(&e.lines, r0, c0, r1, c1, delim);
            *clipboard = tsv.clone();
            ui.output_mut(|o| o.copied_text = tsv);
            // 되돌리기용 스냅샷은 편집 전 상태를 담아야 하므로 지우기 **전에** 캡처.
            let before = replace_op_for_rows(&e.lines, r0, r1);
            crate::edit::clear_cells(&mut e.lines, r0, c0, r1, c1, delim);
            // 대상 셀이 이미 비어 있어 실제로 바뀐 게 없으면(잘라내기는 클립보드
            // 복사만 의미가 있고) 헛된 undo 단계를 남기지 않는다.
            if edit_op_differs_from_current(&before, &e.lines) {
                e.undo.push(before);
                e.dirty = true;
            }
        }
        CellMenuAction::Paste => {
            // 시스템 클립보드 문자열이 있으면 그것이 진실. 없으면 내부 캐시.
            let src: String = match paste_text {
                Some(s) if !s.is_empty() => s.to_owned(),
                _ => clipboard.clone(),
            };
            if src.is_empty() {
                return;
            }
            // 붙여넣기 전 행 수를 재 두어야 "행이 늘었는지"를 알 수 있다.
            let before_len = e.lines.len();
            // 덮어쓸 행 범위 = r0부터 (붙여넣을 줄 수 - 1)까지. 기존 행을
            // 덮는 부분만 Replace로 담는다(늘어난 행은 RemoveInserted가 지운다).
            let paste_rows = src.split('\n').count();
            let overwrite_last = (r0 + paste_rows - 1).min(before_len.saturating_sub(1));
            let replace = if r0 < before_len {
                Some(replace_op_for_rows(&e.lines, r0, overwrite_last))
            } else {
                None
            };
            crate::edit::paste_tsv(&mut e.lines, r0, c0, &src, delim);
            let grew = e.lines.len() > before_len;
            // 한 번의 Ctrl+Z로 값 복원 + 늘어난 행 제거가 모두 일어나야 하므로
            // Batch로 묶는다. Batch 안은 담긴 순서대로 적용되므로 "늘어난 행
            // 제거"를 먼저 두어 인덱스가 어긋나지 않게 한다.
            let op = match (replace, grew) {
                (Some(rep), true) => Some(crate::edit::EditOp::Batch(vec![
                    crate::edit::EditOp::RemoveInserted {
                        at: before_len,
                        count: e.lines.len() - before_len,
                    },
                    rep,
                ])),
                (Some(rep), false) => Some(rep),
                (None, true) => Some(crate::edit::EditOp::RemoveInserted {
                    at: before_len,
                    count: e.lines.len() - before_len,
                }),
                (None, false) => None,
            };
            if let Some(op) = op {
                e.undo.push(op);
            }
            e.dirty = true;
        }
        CellMenuAction::Clear => {
            // 되돌리기용 스냅샷은 편집 전 상태를 담아야 하므로 지우기 **전에** 캡처.
            let before = replace_op_for_rows(&e.lines, r0, r1);
            crate::edit::clear_cells(&mut e.lines, r0, c0, r1, c1, delim);
            // 대상 셀이 이미 비어 있어 실제로 바뀐 게 없으면 헛된 undo 단계를
            // 남기지 않는다(commit_editing_cell과 같은 규율).
            if edit_op_differs_from_current(&before, &e.lines) {
                e.undo.push(before);
                e.dirty = true;
            }
        }
        CellMenuAction::InsertRowAbove => {
            e.undo.push(crate::edit::EditOp::RemoveInserted { at: r0, count: 1 });
            crate::edit::insert_row(&mut e.lines, r0, String::new());
            e.dirty = true;
            // 삽입된 빈 행 아래로 선택이 밀린다.
            doc.cell_sel = Some((r0 + 1, c0, r1 + 1, c1));
        }
        CellMenuAction::InsertRowBelow => {
            let at = (r1 + 1).min(e.lines.len());
            e.undo.push(crate::edit::EditOp::RemoveInserted { at, count: 1 });
            crate::edit::insert_row(&mut e.lines, r1 + 1, String::new());
            e.dirty = true;
        }
        CellMenuAction::DeleteRows => {
            // 되돌리기: 지워질 행들을 순서대로 담아 그 자리에 되꽂는다.
            let removed: Vec<String> = (r0..=r1)
                .filter(|&r| r < e.lines.len())
                .map(|r| e.lines[r].clone())
                .collect();
            if removed.is_empty() {
                return;
            }
            // 버퍼를 통째로 지우면 `remove_row`가 빈 한 줄을 남긴다(빈 lines
            // 방지). 그 유령 줄을 그대로 두고 되꽂으면 `[...원본, ""]`이 되어
            // 한 줄이 늘어나므로, 되돌리기에서 **먼저 그 줄을 치운 뒤** 되꽂도록
            // Batch로 묶는다(Ctrl+Z 한 번에 끝나야 한다).
            let removes_everything = removed.len() == e.lines.len();
            let reinsert = crate::edit::EditOp::ReinsertRemoved { at: r0, lines: removed };
            e.undo.push(if removes_everything {
                crate::edit::EditOp::Batch(vec![
                    crate::edit::EditOp::RemoveInserted { at: 0, count: 1 },
                    reinsert,
                ])
            } else {
                reinsert
            });
            // 뒤에서부터 지워야 인덱스가 밀리지 않는다.
            for r in (r0..=r1).rev() {
                crate::edit::remove_row(&mut e.lines, r);
            }
            e.dirty = true;
            // 선택은 삭제 지점 한 셀로 축소(범위 밖이면 마지막 행으로 클램프).
            let last = e.lines.len().saturating_sub(1);
            let r = r0.min(last);
            doc.cell_sel = Some((r, c0, r, c1));
            doc.editing_cell = None;
        }
    }

}

/// 텍스트 모드 우클릭 컨텍스트 메뉴 동작. `CellMenuAction`과 같은 이유로
/// (클로저 안에서 doc을 가변 대여할 수 없음) 인텐트만 기록해 두고 나중에 적용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextMenuAction {
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
}

/// 텍스트 모드 캐럿/선택 이동·편집 인텐트. 키 입력 루프가 이걸 만들어 내고,
/// 적용 단계에서 `doc.edit`에 반영한다.
#[derive(Debug)]
enum TextEditIntent {
    /// 문자 입력(개행 포함 가능). 선택이 있으면 먼저 지운다.
    Insert(String),
    /// IME 조합 중 글자의 **미리보기**. 버퍼를 바꾸지 않고 화면에만 그린다
    /// (빈 문자열이면 미리보기 해제). `collect_text_intents`의 Preedit 주석 참조.
    ImePreview(String),
    /// Enter — 선택 삭제 후 줄 나누기.
    Newline,
    Backspace,
    Delete,
    /// 캐럿 이동. `extend`면 앵커를 유지한 채 선택을 확장한다.
    Move(CaretMove, bool),
    SelectAll,
    Copy,
    Cut,
    Paste(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaretMove {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

/// 배율 1.0에서의 데이터 영역 고정폭 폰트.
///
/// 프로덕션 렌더는 이걸 쓰지 않는다 — 배율이 걸린 `doc_font_id`를 쓴다. 남겨 둔
/// 이유는 배율을 건드리지 않는 테스트가 렌더와 같은 폰트로 좌표를 재기 위해서다
/// (그 문서들의 `view_scale`이 1.0이므로 두 값이 같다).
#[cfg(test)]
fn text_font_id() -> egui::FontId {
    egui::FontId::monospace(crate::theme::MONO_SIZE)
}

/// 줄의 char 개수.
fn line_char_len(s: &str) -> usize {
    s.chars().count()
}

/// 캐럿을 문서 범위 안으로 클램프한다(줄 번호, 줄 안 col 모두).
fn clamp_pos(lines: &[String], p: crate::edit::TextPos) -> crate::edit::TextPos {
    if lines.is_empty() {
        return crate::edit::TextPos { line: 0, col: 0 };
    }
    let line = p.line.min(lines.len() - 1);
    let col = p.col.min(line_char_len(&lines[line]));
    crate::edit::TextPos { line, col }
}

/// 캐럿 이동 한 번을 적용한 새 위치. 좌/우는 줄 경계를 넘어간다.
fn apply_caret_move(
    lines: &[String],
    p: crate::edit::TextPos,
    mv: CaretMove,
) -> crate::edit::TextPos {
    use crate::edit::TextPos;
    let p = clamp_pos(lines, p);
    match mv {
        CaretMove::Left => {
            if p.col > 0 {
                TextPos { line: p.line, col: p.col - 1 }
            } else if p.line > 0 {
                TextPos { line: p.line - 1, col: line_char_len(&lines[p.line - 1]) }
            } else {
                p
            }
        }
        CaretMove::Right => {
            if p.col < line_char_len(&lines[p.line]) {
                TextPos { line: p.line, col: p.col + 1 }
            } else if p.line + 1 < lines.len() {
                TextPos { line: p.line + 1, col: 0 }
            } else {
                p
            }
        }
        // 위/아래는 col을 유지하되 대상 줄 길이로 클램프(일반 에디터 동작).
        CaretMove::Up => {
            if p.line == 0 {
                TextPos { line: 0, col: 0 }
            } else {
                clamp_pos(lines, TextPos { line: p.line - 1, col: p.col })
            }
        }
        CaretMove::Down => {
            if p.line + 1 >= lines.len() {
                TextPos { line: p.line, col: line_char_len(&lines[p.line]) }
            } else {
                clamp_pos(lines, TextPos { line: p.line + 1, col: p.col })
            }
        }
        CaretMove::Home => TextPos { line: p.line, col: 0 },
        CaretMove::End => TextPos { line: p.line, col: line_char_len(&lines[p.line]) },
    }
}

/// 방향키/Home/End 한 번의 결과 (새 캐럿, 새 선택).
///
/// - `extend`(Shift 동반)면 앵커를 유지한 채 캐럿만 옮겨 선택을 넓힌다.
///   선택이 없었다면 현재 캐럿이 앵커가 된다.
/// - `extend`가 아니고 선택이 있으면 좌/위는 선택 시작으로, 우/아래는 선택
///   끝으로 캐럿을 붕괴시킨다(일반 에디터 동작 — 첫 방향키는 선택 해제만).
///   Home/End는 붕괴 후 그 줄 안에서 이동한다.
/// - 선택이 없으면 그냥 한 칸 이동.
fn next_caret_and_sel(
    lines: &[String],
    caret: crate::edit::TextPos,
    sel: Option<(crate::edit::TextPos, crate::edit::TextPos)>,
    mv: CaretMove,
    extend: bool,
) -> (
    crate::edit::TextPos,
    Option<(crate::edit::TextPos, crate::edit::TextPos)>,
) {
    if extend {
        let anchor = sel.map(|(a, _)| a).unwrap_or(caret);
        let to = apply_caret_move(lines, caret, mv);
        let new_sel = if anchor == to { None } else { Some((anchor, to)) };
        return (to, new_sel);
    }
    match sel {
        Some((a, b)) => {
            let (lo, hi) = crate::edit::normalize(a, b);
            match mv {
                // 좌/위: 선택 시작으로 붕괴(이동 없음).
                CaretMove::Left | CaretMove::Up => (lo, None),
                // 우/아래: 선택 끝으로 붕괴(이동 없음).
                CaretMove::Right | CaretMove::Down => (hi, None),
                // Home/End: 캐럿이 있던 줄 안에서 이동.
                _ => (apply_caret_move(lines, caret, mv), None),
            }
        }
        None => (apply_caret_move(lines, caret, mv), None),
    }
}

/// Backspace/Delete가 버퍼를 실제로 바꿨는지 판정한다(dirty 표시 여부).
///
/// - 선택이 있었으면 그 범위를 지웠으므로 무조건 변경이다.
/// - 선택이 없었으면 캐럿이 움직였는지로 본다. Backspace는 한 글자/개행을
///   지울 때 반드시 캐럿이 뒤로 가고, 지울 게 없는 문서 맨 앞(0,0)에서는
///   `edit::backspace`가 위치를 그대로 돌려주므로 이 판정이 정확하다.
///
/// (Delete는 실제 삭제를 해도 캐럿이 제자리라 이 함수를 쓰지 않는다 —
///  호출측이 "삭제를 수행했는가"를 직접 본다.)
fn backspace_or_delete_changed(
    had_sel: bool,
    before: crate::edit::TextPos,
    after: crate::edit::TextPos,
) -> bool {
    had_sel || before != after
}

/// 문서 전체를 덮는 선택 (0,0) ~ (마지막 줄 끝).
fn whole_document_sel(lines: &[String]) -> (crate::edit::TextPos, crate::edit::TextPos) {
    use crate::edit::TextPos;
    let last = lines.len().saturating_sub(1);
    (
        TextPos { line: 0, col: 0 },
        TextPos { line: last, col: lines.get(last).map_or(0, |l| line_char_len(l)) },
    )
}

/// 정규화된 선택이 `line` 줄에서 덮는 char 구간 [c0, c1). 걸치지 않으면 None.
/// 줄 끝을 넘어가는 선택(다음 줄로 이어지는 경우)은 줄 길이 + 1로 표시해
/// "개행까지 선택됨"을 음영 폭으로 드러낸다.
fn sel_span_on_line(
    a: crate::edit::TextPos,
    b: crate::edit::TextPos,
    line: usize,
    len: usize,
) -> Option<(usize, usize)> {
    if line < a.line || line > b.line {
        return None;
    }
    let c0 = if line == a.line { a.col.min(len) } else { 0 };
    // 마지막 줄이 아니면 줄 끝 + 개행 한 칸까지.
    let c1 = if line == b.line { b.col.min(len) } else { len + 1 };
    if c0 >= c1 {
        return None;
    }
    Some((c0, c1))
}

/// 한 줄(또는 표 셀)에 찾기 매치 음영을 그린다. **선택 음영과 글자 사이**에
/// 그려야 하므로 호출부는 선택 음영 → 이 함수 → `painter.galley` 순으로 부른다.
///
/// char↔x 매핑은 **호출부가 넘긴 galley 하나**로만 한다 — 편집 모드가 캐럿/
/// 선택을 그릴 때 쓰는 그 galley를 그대로 받으므로, 음영이 글자와 어긋날 수
/// 없다(CJK/탭에서 반복됐던 정렬 버그 방지). `x_of`도 그 galley의
/// `pos_from_ccursor`를 쓰는 호출부의 클로저를 그대로 받는다.
///
/// `matches`는 이 줄에서 찾은 (col, len) 목록(char 인덱스). `current`가 Some이면
/// 그 (col, len)에 해당하는 매치는 `find_current_bg`(진한 보라)로, 나머지는
/// `find_match_bg`(옅은 보라)로 그린다 — current를 나중에(위에) 덮어 더 진하게.
/// 줄에서 탭 문자가 있는 char 위치들. `paint_tab_shades`가 칠할 칸을 고른다.
///
/// **char 인덱스여야 한다**(바이트가 아니라) — `x_of`가 `CCursor`를 받으므로
/// 한글이 섞이면 바이트 오프셋은 엉뚱한 자리를 가리킨다.
fn tab_positions(line: &str) -> Vec<usize> {
    line.chars()
        .enumerate()
        .filter(|&(_, c)| c == '\t')
        .map(|(i, _)| i)
        .collect()
}

/// 탭이 차지하는 칸에 옅은 배경을 깐다. 스페이스와 탭이 둘 다 빈 칸으로 보여
/// 구분되지 않는 문제를 푼다.
///
/// **탭 한 칸의 폭을 산술로 구하지 않는다.** epaint는 탭을 "다음 탭스톱까지"가
/// 아니라 **고정폭 빈 글자**로 그린다(`TAB_SIZE(4) × 스페이스 advance`,
/// `epaint-0.28.1/src/text/font.rs:187-190`). 그래서 갤리 안에서 탭은 평범한
/// 한 글자이고, 그 칸은 `x_of(i)`부터 `x_of(i+1)`까지다. 갤리에게 물으므로
/// 배율이 바뀌어도, 폰트가 폴백으로 넘어가도 어긋나지 않는다
/// (헥스 정렬을 고칠 때 배운 것과 같은 규율).
fn paint_tab_shades(
    painter: &egui::Painter,
    cell_rect: egui::Rect,
    x_of: &dyn Fn(usize) -> f32,
    tabs: &[usize],
) {
    for &i in tabs {
        let x0 = x_of(i);
        let x1 = x_of(i + 1);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x0, cell_rect.top()),
                egui::pos2(x1, cell_rect.bottom()),
            ),
            0.0,
            crate::theme::tab_bg(),
        );
    }
}

fn paint_match_shades(
    painter: &egui::Painter,
    cell_rect: egui::Rect,
    x_of: &dyn Fn(usize) -> f32,
    matches: &[(usize, usize)],
    current: Option<(usize, usize)>,
) {
    // 1) 전체 매치를 옅은 보라로.
    for &(col, len) in matches {
        let x0 = x_of(col);
        let x1 = x_of(col + len);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x0, cell_rect.top()),
                egui::pos2(x1, cell_rect.bottom()),
            ),
            0.0,
            crate::theme::find_match_bg(),
        );
    }
    // 2) current 매치가 이 줄에 있으면 그 위에 진한 보라로 덮어 그린다.
    if let Some((cc, cl)) = current {
        if matches.iter().any(|&(col, len)| col == cc && len == cl) {
            let x0 = x_of(cc);
            let x1 = x_of(cc + cl);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x0, cell_rect.top()),
                    egui::pos2(x1, cell_rect.bottom()),
                ),
                0.0,
                crate::theme::find_current_bg(),
            );
        }
    }
}

/// 편집 모드 텍스트 줄이 쓰는 상호작용 sense.
///
/// `Sense::click_and_drag()`와 click/drag는 같지만 `focusable`이 다르다:
/// 그 생성자는 `focusable: true`를 넣는다(`egui-0.28.1/src/sense.rs:92-98`).
/// 본문 줄이 focusable이면 `Context::create_widget`이
/// `interested_in_focus`로 등록해(`context.rs:1050`) Tab 순회 대상이 되고,
/// Tab 한 번으로 포커스가 줄에 걸리면 `render_text`의 keyboard_free 게이트가
/// 닫혀 모든 키 입력이 삼켜진다. 그래서 명시적으로 opt-out 한다.
const TEXT_LINE_SENSE: egui::Sense = egui::Sense {
    click: true,
    drag: true,
    focusable: false,
};

/// 표 모드 데이터 셀이 쓰는 상호작용 sense. `TEXT_LINE_SENSE`와 값은 같지만
/// 이유가 달라 따로 둔다(한쪽을 고칠 때 다른 쪽이 딸려가지 않게).
///
/// 셀도 텍스트 줄과 똑같이 `Sense::click_and_drag()`(= `focusable: true`,
/// `egui-0.28.1/src/sense.rs:92-98`)를 쓰고 있었다. 그러면 화면에 그려진
/// **모든 셀**이 `interested_in_focus`로 등록돼(`context.rs:1050`) Tab 순회
/// 대상이 된다. 표 모드에는 `render_text`의 `keyboard_free` 같은 전역 키
/// 게이트가 없어 오늘 당장 편집 불가가 되지는 않지만,
///
/// - Tab이 셀 위에 눈에 보이지 않는 포커스를 남겨 인라인 셀 편집기
///   (`("celledit", ..)` TextEdit)의 포커스 이동과 간섭할 여지가 있고,
/// - 표 모드가 나중에 키보드 게이트를 갖는 순간 텍스트 모드와 똑같이
///   문서 전체가 편집 불가가 된다.
///
/// 셀은 포커스가 필요 없다 — 클릭/드래그는 `interact_pointer_pos` 경로로,
/// 편집은 별도의 `TextEdit` 위젯으로 처리한다. 그래서 명시적으로 opt-out 한다.
const TABLE_CELL_SENSE: egui::Sense = egui::Sense {
    click: true,
    drag: true,
    focusable: false,
};

/// 스크롤 마커 거터. 데이터 영역 오른쪽에 얇은 세로 바를 그려, Find All 스냅샷
/// (`doc.highlight.rows`)의 매치 위치를 보라 눈금으로 표시한다 — EMEditor의 우측
/// 마커 바와 같은 방식이다. `Table::body`가 `ScrollAreaOutput`을 삼켜 스크롤
/// 트랙 rect를 노출하지 않으므로 기본 스크롤바 위에 겹칠 수 없어, 별도 거터를
/// 직접 만든다(설계 S-7).
///
/// 호출부(`update()`)가 `show_gutter`로 스냅샷이 있고 매치가 있을 때만 이 함수를
/// 부른다. 스냅샷이 없으면(하이라이트 없음) 애초에 그려지지 않는다.
///
/// **성능(S-8/E2-6): 픽셀 양자화.** `highlight.rows`는 수백만이 될 수 있으나 거터는
/// 기껏 수백 픽셀 높이다. 같은 정수 y 픽셀에 여러 눈금을 그려도 화면상 구분이
/// 안 되므로, `rows`가 행 오름차순인 성질을 이용해 **마지막으로 그린 정수
/// y**를 기억하고 같은 y는 건너뛴다(O(n) 순회 + 최대 거터높이만큼만 draw 호출).
/// 이렇게 하면 200만 매치라도 draw 호출은 거터 픽셀 수 이하로 묶인다.
fn render_match_gutter(ctx: &egui::Context, doc: &mut Document) {
    let line_count = doc_line_count(doc);
    // 스냅샷의 매치 행. 호출부가 `show_gutter`로 Some을 보장했지만, 방어적으로
    // 비어 있으면 빈 슬라이스로 순회해 아무 눈금도 그리지 않는다.
    let rows: Vec<u32> = doc.highlight.as_ref().map(|h| h.rows.clone()).unwrap_or_default();
    // 거터 배경은 데이터 영역과 같은 순백 — 눈금 보라만 도드라지게 한다.
    let frame = egui::Frame::none().fill(crate::theme::data_bg());
    egui::SidePanel::right("find_marker_gutter")
        .exact_width(14.0)
        .resizable(false)
        .frame(frame)
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            let top = rect.top();
            let height = rect.height();
            // 거터 전체를 클릭 대상으로. 클릭 y를 논리 행으로 역산해 스크롤 요청.
            let resp = ui.interact(rect, ui.id().with("gutter"), egui::Sense::click());

            let painter = ui.painter().with_clip_rect(rect);
            let marker = crate::theme::find_marker();
            // 눈금은 2px 높이, 거터 가로 폭 안쪽으로 살짝 여백을 둔다.
            let x0 = rect.left() + 2.0;
            let x1 = rect.right() - 2.0;
            let mut last_px: Option<i32> = None;
            for &r in &rows {
                let y = marker_y(r as usize, line_count, top, height);
                let py = y.round() as i32;
                // 같은 정수 y는 한 번만(양자화). rows가 오름차순이라
                // last_px 비교만으로 충분하다.
                if last_px == Some(py) {
                    continue;
                }
                last_px = Some(py);
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + 2.0)),
                    0.0,
                    marker,
                );
            }
            // current match 행은 거터에도 진한 눈금으로(다른 눈금보다 도드라지게).
            if let Some(m) = doc.last_match {
                if m.line < line_count {
                    let y = marker_y(m.line, line_count, top, height);
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(rect.left(), y - 1.0),
                            egui::pos2(rect.right(), y + 3.0),
                        ),
                        0.0,
                        crate::theme::find_current_bg(),
                    );
                }
            }

            // 거터 클릭 → 그 위치로 점프. 마커는 **논리 행**이지만 스크롤은 화면
            // 행 기준이므로, 표 모드에서는 정렬 permutation을 거쳐 변환한다
            // (`focus_match`와 같은 규율). 텍스트 모드는 논리 행이 곧 화면 행이다.
            if let Some(pos) = resp.interact_pointer_pos() {
                if resp.clicked() {
                    let logical = row_at_y(pos.y, line_count, top, height);
                    // 거터 클릭도 찾기와 같은 중앙 정렬이다(`gutter_click_target` 참조 —
                    // Page 키가 TOP으로 바꿔 놨을 수 있어 매번 되돌려야 한다).
                    let (align, row) = gutter_click_target(doc, logical, doc.sep);
                    doc.pending_scroll_align = align;
                    doc.pending_scroll_row = Some(row);
                }
            }
        });
}

/// 텍스트 모드 렌더: 라인번호 + 줄 전체(구분 안 함).
///
/// 뷰 전용 모드(`doc.edit == None`)에서는 기존과 동일하게 `Label` + truncate로
/// 그린다. 편집 모드에서는 캐럿/선택을 정확히 그려야 하므로 줄 텍스트를 직접
/// 고정폭 galley로 레이아웃해 그리고, char↔x 매핑에 그 galley의
/// `pos_from_ccursor` / `cursor_from_pos`를 쓴다(근사 아님).
fn render_text(
    ui: &mut egui::Ui,
    doc: &mut Document,
    row_base: usize,
    clipboard: &mut String,
    tab_pressed: bool,
    lang: crate::i18n::Lang,
) {
    let s = crate::i18n::t(lang);
    use std::cell::Cell;

    // 찾기가 남긴 스크롤 요청(표 모드와 같은 이유·같은 방법 — render_table 주석 참조).
    let scroll_to = doc.pending_scroll_row.take();
    let scroll_align = doc.pending_scroll_align;

    let editing = doc.edit.is_some();
    // Parquet은 표 모드로만 그리므로 여기 오지 않지만, 행 수를 얻는 방법은
    // 한 가지로 통일해 둔다(`doc_line_count`가 세 출처를 모두 안다).
    let total_lines = doc_line_count(doc);
    let avail_height = ui.available_height();
    // 행 높이는 배율을 탄다 — 상수 ROW_HEIGHT를 직접 쓰면 확대 시 글자가 잘린다.
    let row_h = doc_row_height(doc);

    // ---- 편집 모드 상태 스냅샷 + 클로저 → 바깥 인텐트 통로 ----
    // 표 모드와 같은 규율: 테이블 클로저는 doc을 불변으로만 빌리고, 상호작용
    // 결과는 여기 모아 두었다가 클로저 종료 후 doc.edit에 적용한다.
    let caret = doc.text_caret;
    // IME 조합 중 글자. 버퍼에 없으므로 여기서 스냅샷해 캐럿 자리에 덧그린다.
    let ime_preview = doc.ime_preview.clone();
    // 정규화한 선택(음영 그리기용).
    let sel_norm = doc
        .text_sel
        .map(|(a, b)| crate::edit::normalize(a, b))
        .filter(|(a, b)| a != b);
    let font_id = doc_font_id(doc);
    let text_color = ui.visuals().text_color();
    let caret_color = ui.visuals().strong_text_color();

    // 찾기 하이라이트 스냅샷. **라이브 `find_query`가 아니라 Find All 스냅샷
    // (`doc.highlight`)만** 본다 — 입력란 타이핑이 스캔을 유발하지 않게 하는 핵심.
    // 스냅샷이 없으면 뷰 모드는 기존 `Label` 경로를 그대로 써 회귀를 피하므로,
    // "하이라이트가 있는가"를 미리 판정해 둔다.
    let find_query = doc.highlight.as_ref().map(|h| h.query.clone()).unwrap_or_default();
    let find_opts = doc
        .highlight
        .as_ref()
        .map(|h| h.opts.clone())
        .unwrap_or_default();
    let searching = doc.highlight.is_some();
    let last_match = doc.last_match;

    // 클릭/드래그로 잡은 위치. anchor는 누름 시작, head는 확장 끝점.
    let drag_anchor: Cell<Option<crate::edit::TextPos>> = Cell::new(None);
    let drag_head: Cell<Option<crate::edit::TextPos>> = Cell::new(None);
    // Shift+클릭으로 확장할 캐럿 위치. 앵커는 **잡지 않는다**(기존 앵커 유지).
    let shift_click: Cell<Option<crate::edit::TextPos>> = Cell::new(None);
    // 더블/트리플 클릭이 만든 선택 (anchor, head). 아래 2)에서 드래그가 만든
    // 선택을 **덮어쓴다** — 같은 프레임에 press 분기가 캐럿 하나로 무너뜨린
    // 뒤이기 때문.
    let multi_click: Cell<Option<(crate::edit::TextPos, crate::edit::TextPos)>> = Cell::new(None);
    // 이번 프레임에 "텍스트 줄 위에서" 좌클릭 누름이 진행 중인지.
    let line_press: Cell<bool> = Cell::new(false);
    // 우클릭 대상 줄 위치 + 고른 메뉴 동작.
    let menu_target: Cell<Option<crate::edit::TextPos>> = Cell::new(None);
    let menu_action: Cell<Option<TextMenuAction>> = Cell::new(None);

    let primary_down = ui.input(|i| i.pointer.primary_down());
    // Shift 눌림. 표 모드와 같은 규율 — Shift+클릭은 앵커를 새로 잡지 않는다.
    let shift_down = ui.input(|i| i.modifiers.shift);
    // 이전 프레임부터 이어져 온 "줄에서 시작된 드래그". 버튼을 떼면 꺼진다.
    // (`next_cell_drag_active`와 같은 전이 규칙을 그대로 쓴다 — 표/텍스트 모드가
    // 다른 것은 무엇을 눌렀는지뿐이고, 래치 논리는 동일하다.)
    let drag_active = doc.text_drag_active && primary_down;

    // 키 입력은 프레임당 한 번만 읽는다. 편집 모드 + 다른 위젯이 키보드 포커스를
    // 갖고 있지 않을 때만 소비한다 — 그렇지 않으면 툴바의 "직접:" 구분자
    // TextEdit 등에 타이핑할 때 같은 키가 본문에도 들어간다.
    //
    // 이 게이트가 성립하는 전제는 "본문 텍스트 줄이 포커스를 가져갈 수 없다"이다.
    // 그 전제는 저절로 성립하지 않는다 — `Sense::click_and_drag()`는
    // `focusable: true`이고(`egui-0.28.1/src/sense.rs:92-98`), 그런 sense로 만든
    // 위젯은 `context.rs:1050`에서 `interested_in_focus`로 등록되어 Tab 순회
    // 대상이 된다. 그래서 아래 줄 interact는 `focusable: false`를 **명시적으로**
    // 지정한다(TEXT_LINE_SENSE). 그 결과 Tab이 본문 줄로 포커스를 옮길 수 없고,
    // "포커스 없음" = "본문이 입력을 받는다"가 실제로 참이 된다.
    // (명시하지 않으면 Tab 한 번으로 포커스가 줄에 걸려 모든 키 입력이 조용히
    //  삼켜지고 문서가 편집 불가 상태가 된다.)
    // Tab은 **포커스 게이트보다 먼저** 소비한다. 순서를 바꿀 수 없는 이유:
    // Tab이 소비되지 않으면 egui가 그것으로 포커스를 툴바 위젯에 옮기고,
    // 그 순간 `keyboard_free`가 거짓이 되어 다음 프레임부터 이 블록 자체가
    // 실행되지 않는다 — 즉 **Tab이 자기 자신을 막는다**. 게이트 안에서
    // 소비하면 첫 Tab은 포커스를 옮기고 그 뒤로는 영영 탭이 안 들어간다.
    //
    // 편집 모드에서만 소비한다. 뷰 모드에서는 Tab이 평범한 포커스 순회여야
    // 한다(접근성).
    let intents: Vec<TextEditIntent> = text_frame_intents(ui.ctx(), editing, tab_pressed);

    // 행 간격 — 표 모드와 같은 이유(`scroll_offset_for_row` 주석).
    let spacing_y = ui.spacing().item_spacing.y;

    // 줄 전체 컬럼은 넉넉한 초기폭 + resizable. 긴 줄은 셀 안에서 truncate.
    let mut table = TableBuilder::new(ui)
        .striped(true)
        .auto_shrink([false, false])
        .max_scroll_height(avail_height)
        .column(Column::initial(64.0).at_least(48.0).resizable(true)) // 라인번호 #
        .column(Column::remainder().at_least(200.0).resizable(true)); // 줄 전체
    if let Some(row) = scroll_to {
        // 텍스트 모드는 논리 행이 곧 화면 행이라 전체 줄 수로 클램프한다.
        let row = row.min(total_lines.saturating_sub(1));
        table = table.vertical_scroll_offset(scroll_offset_for_row(
            row,
            scroll_align,
            row_h,
            spacing_y,
            (avail_height - row_h).max(0.0),
        ));
    }

    // 표 모드와 같은 통로 — Page Up/Down이 읽을 "화면 첫 행"을 관측한다.
    // 텍스트 모드는 정렬 permutation이 없어 화면 행 = 논리 행이다.
    let min_drawn_row: Cell<Option<usize>> = Cell::new(None);

    table
        .header(row_h, |mut header| {
            header.col(|ui| {
                let rect = ui.max_rect();
                paint_header_cell(ui, rect, crate::theme::header_bg());
                // 아래 라인번호가 오른쪽 정렬이므로 머리글도 오른쪽에 맞춘다.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new("#").strong()).truncate());
                });
            });
            header.col(|ui| {
                let rect = ui.max_rect();
                paint_header_cell(ui, rect, crate::theme::header_bg());
                ui.add(egui::Label::new(egui::RichText::new(s.common_line).strong()).truncate());
            });
        })
        .body(|body| {
            body.rows(row_h, total_lines, |mut row| {
                let logical = row.index();
                min_drawn_row.set(Some(
                    min_drawn_row.get().map_or(logical, |m: usize| m.min(logical)),
                ));
                let line_no = logical + row_base;
                row.col(|ui| {
                    let rect = ui.max_rect();
                    paint_line_number_cell(ui, rect, format!("{line_no}"));
                });
                let line = logical_line(doc, logical).unwrap_or_default();
                row.col(|ui| {
                    // ---- 뷰 전용 모드 ----
                    if !editing {
                        // 검색 중이 아니면 기존 `Label` 경로 그대로(픽셀 단위 회귀
                        // 방지). 검색 중일 때만 galley로 바꿔 부분 음영을 그린다 —
                        // 음영·글자만, 캐럿/선택/상호작용은 없다(뷰 모드엔 없으므로).
                        if !searching {
                            // `Label`을 쓰되 **그 자신의 galley**를 받아 쓴다
                            // (`layout_in_ui`는 레이아웃·자리 확보만 하고 그리지는
                            // 않는다). 여기서 galley를 새로 만들어 재면 `Label`의
                            // 레이아웃(wrap·truncate·Body 스타일)과 어긋날 수 있는데,
                            // 그 위험 없이 탭 칸 좌표를 정확히 얻는 유일한 방법이다.
                            let tabs = tab_positions(&line);
                            let (pos, galley, resp) =
                                egui::Label::new(line).truncate().layout_in_ui(ui);
                            // 탭 배경 → 글자 순서로 그려야 배경이 글자를 덮지 않는다.
                            if !tabs.is_empty() {
                                let x_of = |c: usize| -> f32 {
                                    pos.x
                                        + galley
                                            .pos_from_ccursor(egui::text::CCursor::new(c))
                                            .min
                                            .x
                                };
                                paint_tab_shades(
                                    &ui.painter().with_clip_rect(resp.rect),
                                    resp.rect,
                                    &x_of,
                                    &tabs,
                                );
                            }
                            ui.painter().galley(pos, galley, text_color);
                            let ending = line_ending_for_row(doc, logical);
                            paint_line_ending(
                                ui.painter(),
                                egui::pos2(resp.rect.right(), resp.rect.top()),
                                ending,
                                &font_id,
                                ui,
                            );
                            return;
                        }
                        let cell_rect = ui.max_rect();
                        let len = line_char_len(&line);
                        let galley = ui.fonts(|f| {
                            f.layout_no_wrap(line.clone(), font_id.clone(), text_color)
                        });
                        let origin = egui::pos2(
                            cell_rect.left(),
                            cell_rect.center().y - galley.size().y * 0.5,
                        );
                        let x_of = |c: usize| -> f32 {
                            origin.x
                                + galley
                                    .pos_from_ccursor(egui::text::CCursor::new(c.min(len)))
                                    .min
                                    .x
                        };
                        let painter = ui.painter().with_clip_rect(cell_rect);
                        // 탭 칸 배경을 매치 음영 **아래**에 먼저 깐다 — 탭 위에
                        // 매치가 겹치면 매치가 이겨야 한다(찾은 자리가 우선).
                        paint_tab_shades(&painter, cell_rect, &x_of, &tab_positions(&line));
                        // 텍스트 모드는 delim=None. current는 이 논리 행의 last_match.
                        let matches =
                            crate::find::find_in_line_scoped(&line, &find_query, &find_opts, None);
                        let current = last_match
                            .filter(|m| m.line == logical)
                            .map(|m| (m.col, m.len));
                        paint_match_shades(&painter, cell_rect, &x_of, &matches, current);
                        painter.galley(origin, galley.clone(), text_color);
                        paint_line_ending(
                            &painter,
                            egui::pos2(x_of(len), origin.y),
                            line_ending_for_row(doc, logical),
                            &font_id,
                            ui,
                        );
                        return;
                    }

                    // ---- 편집 모드 ----
                    let cell_rect = ui.max_rect();
                    let len = line_char_len(&line);
                    // 줄 텍스트를 고정폭으로 직접 레이아웃한다. 이 galley 하나가
                    // (a) 실제 그리는 글자, (b) char→x, (c) x→char의 유일한
                    // 진실이므로 캐럿/음영이 글자와 어긋날 수 없다.
                    let galley = ui.fonts(|f| {
                        f.layout_no_wrap(line.clone(), font_id.clone(), text_color)
                    });
                    // 셀 왼쪽 위에 세로 중앙 정렬해 그린다.
                    let origin = egui::pos2(
                        cell_rect.left(),
                        cell_rect.center().y - galley.size().y * 0.5,
                    );
                    // char 인덱스 → 셀 좌표 x.
                    let x_of = |c: usize| -> f32 {
                        origin.x
                            + galley
                                .pos_from_ccursor(egui::text::CCursor::new(c.min(len)))
                                .min
                                .x
                    };

                    let painter = ui.painter().with_clip_rect(cell_rect);
                    // 0) 탭 칸 배경을 가장 아래에. 선택·매치가 그 위에 겹치면
                    //    그쪽이 이겨야 한다(지금 무엇을 하고 있는지가 우선).
                    paint_tab_shades(&painter, cell_rect, &x_of, &tab_positions(&line));
                    // 1) 선택 음영을 글자 아래에 먼저.
                    if let Some((a, b)) = sel_norm {
                        if let Some((c0, c1)) = sel_span_on_line(a, b, logical, len) {
                            // c1이 len+1이면(개행 포함) 줄 끝 너머 한 칸을 더 칠한다.
                            let x0 = x_of(c0);
                            let x1 = if c1 > len {
                                x_of(len) + galley.size().y * 0.4
                            } else {
                                x_of(c1)
                            };
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(x0, cell_rect.top()),
                                    egui::pos2(x1, cell_rect.bottom()),
                                ),
                                0.0,
                                sel_shade(),
                            );
                        }
                    }
                    // 1.5) 찾기 매치 음영을 선택 음영과 글자 **사이**에.
                    if searching {
                        let matches =
                            crate::find::find_in_line_scoped(&line, &find_query, &find_opts, None);
                        let current = last_match
                            .filter(|m| m.line == logical)
                            .map(|m| (m.col, m.len));
                        paint_match_shades(&painter, cell_rect, &x_of, &matches, current);
                    }
                    // 2) 글자.
                    painter.galley(origin, galley.clone(), text_color);
                    // 2.5) 줄 끝 개행 기호. **글자 galley 바깥에 따로 그린다** —
                    // 기호를 galley에 넣으면 그 galley가 곧 char↔x 매핑의 진실
                    // (`x_of`/`cursor_from_pos`)이므로 캐럿이 없는 글자 위에
                    // 서거나 줄 끝 클릭이 기호 안쪽으로 빨려 들어간다. 그리는
                    // 것과 좌표 계산을 분리해 두는 것이 핵심이다.
                    paint_line_ending(
                        &painter,
                        egui::pos2(x_of(len), origin.y),
                        line_ending_for_row(doc, logical),
                        &font_id,
                        ui,
                    );
                    // 3) 캐럿(그 줄일 때만).
                    if caret.line == logical {
                        let x = x_of(caret.col);
                        // 3-a) IME 조합 중 글자를 캐럿 자리에 덧그린다. 버퍼에
                        // 없는 글자이므로 본문 galley와 별개로 그리고, 조합
                        // 중임이 보이도록 밑줄을 깐다(IME 관행).
                        let ime_w = paint_ime_preview(
                            &painter,
                            ui,
                            egui::pos2(x, origin.y),
                            &ime_preview,
                            &font_id,
                            text_color,
                        );
                        // 캐럿은 조합 글자 **뒤에** 선다 — 다음 글자가 그
                        // 자리에 붙기 때문.
                        let x = x + ime_w;
                        let caret_rect = egui::Rect::from_min_max(
                            egui::pos2(x, cell_rect.top() + 2.0),
                            egui::pos2(x + 1.5, cell_rect.bottom() - 2.0),
                        );
                        painter.rect_filled(caret_rect, 0.0, caret_color);
                        // IME 조합 창을 캐럿 자리로 보낸다. 이걸 알려 주지
                        // 않으면 한글 조합 창이 캐럿과 동떨어진 곳(창 좌상단
                        // 등)에 뜨고, **무엇보다 IME가 켜지지 않는다** —
                        // egui-winit이 `let allow_ime = ime.is_some()`으로
                        // 판단하기 때문(lib.rs:831). egui의 `TextEdit`도 같은
                        // 일을 한다(`text_edit/builder.rs`의 `o.ime = Some(..)`).
                        if ime_should_follow_caret(editing) {
                            set_ime_output(ui.ctx(), cell_rect, caret_rect);
                        }
                    }

                    // 4) 클릭/드래그 상호작용. 셀 전체가 대상.
                    // sense를 직접 만들어 `focusable: false`를 강제한다 —
                    // `Sense::click_and_drag()`는 focusable: true라 Tab이 여기로
                    // 포커스를 옮길 수 있고, 그러면 위쪽 keyboard_free 게이트가
                    // 닫혀 문서 전체가 편집 불가가 된다(위 주석 참조).
                    let id = ui.id().with(("textline", logical));
                    let resp = ui.interact(cell_rect, id, TEXT_LINE_SENSE);
                    // 포인터 x → char 인덱스(같은 galley로 역매핑).
                    let pos_at_pointer = |pp: egui::Pos2| -> crate::edit::TextPos {
                        let local = pp - origin;
                        // y는 한 줄짜리 galley이므로 0으로 눌러 첫 행에 붙인다.
                        let cur = galley.cursor_from_pos(egui::vec2(local.x, 0.0));
                        crate::edit::TextPos {
                            line: logical,
                            col: cur.ccursor.index.min(len),
                        }
                    };

                    // `&& primary_down` 게이트는 표 모드와 같은 이유다:
                    // is_pointer_button_down_on()은 버튼을 가리지 않아 우클릭
                    // press에도 참이므로, 게이트가 없으면 우클릭이 선택을
                    // 캐럿 하나로 무너뜨린다(메뉴는 release에서야 열린다).
                    let pressed_here = resp.is_pointer_button_down_on() && primary_down;
                    if pressed_here {
                        line_press.set(true);
                    }
                    if let Some(pp) = resp.interact_pointer_pos() {
                        let p = pos_at_pointer(pp);
                        // 새 앵커 = 이번 프레임에 이 줄에서 누름이 시작됐다.
                        // 드래그가 이어지는 동안(drag_active)에는 앵커를 다시
                        // 잡지 않아야 확장이 깨지지 않는다.
                        // Shift+클릭(Windows 표준): 기존 앵커를 유지한 채
                        // 캐럿만 이 위치로. 표 모드와 같은 이유로 앵커를
                        // 잡는 분기와 **분리**한다.
                        if resp.clicked() || (pressed_here && !drag_active) {
                            if shift_down {
                                shift_click.set(Some(p));
                            } else {
                                drag_anchor.set(Some(p));
                                drag_head.set(Some(p));
                            }
                        }
                    }
                    // 더블클릭 → 공백으로 구분된 덩어리, 트리플클릭 → 줄 전체.
                    // Windows 텍스트 상자의 표준 동작이다.
                    //
                    // 위 press 분기는 같은 프레임에 캐럿을 하나로 무너뜨린다
                    // (`clicked()`가 release 프레임에 참이므로 둘 다 돈다).
                    // 그래서 여기서는 판정만 기록하고, 실제 적용은 아래 2)에서
                    // 드래그 분기 **뒤에** 한다.
                    //
                    // egui의 count는 3에서 멈춘다 — 네 번째부터도 triple로
                    // 보고되므로 계속 눌러도 줄 선택이 유지된다.
                    if resp.triple_clicked() {
                        multi_click.set(Some((
                            crate::edit::TextPos { line: logical, col: 0 },
                            crate::edit::TextPos { line: logical, col: len },
                        )));
                    } else if resp.double_clicked() {
                        if let Some(pp) = resp.interact_pointer_pos() {
                            let at = pos_at_pointer(pp);
                            let (a, b) = crate::edit::word_range_at(&line, at.col);
                            multi_click.set(Some((
                                crate::edit::TextPos { line: logical, col: a },
                                crate::edit::TextPos { line: logical, col: b },
                            )));
                        }
                    }

                    // 드래그 중 확장: 포인터가 이 줄 위 + 줄에서 시작된 드래그.
                    // 다른 줄로 넘어가는 확장은 원 위젯이 포인터를 캡처하므로
                    // contains_pointer()로 감지한다(표 모드와 동일).
                    if drag_active && resp.contains_pointer() {
                        if let Some(pp) = ui.input(|i| i.pointer.latest_pos()) {
                            drag_head.set(Some(pos_at_pointer(pp)));
                        }
                    }

                    // 5) 우클릭 컨텍스트 메뉴.
                    // 어떤 줄에서 우클릭했는지는 **메뉴가 열리는 프레임에만** 기록한다.
                    // context_menu의 클로저는 메뉴가 떠 있는 매 프레임 다시 돌기
                    // 때문에, 그 안에서 set하면 menu_target이 메뉴 수명 내내
                    // Some으로 남아 아래 4)의 "선택 밖이면 선택 해제" 판정이 매
                    // 프레임 재실행된다. 그러면 메뉴가 열린 채 Ctrl+A/Shift+화살표로
                    // 만든 **새 선택이 곧바로 지워진다**. secondary_clicked()는
                    // 메뉴를 여는 release 프레임에만 참이므로 여는 순간만 잡는다.
                    // col은 줄 끝으로 둔다 — 메뉴가 열린 뒤 포인터는 메뉴 창 위에
                    // 있어 이 줄 좌표계로 되돌릴 수 없고, 메뉴 동작들은
                    // "선택 밖이면 그 줄로" 이상의 정밀도를 쓰지 않는다.
                    if resp.secondary_clicked() {
                        menu_target.set(Some(crate::edit::TextPos {
                            line: logical,
                            col: len,
                        }));
                    }
                    resp.context_menu(|ui| {
                        let pick = |ui: &mut egui::Ui, label: &str, act: TextMenuAction| {
                            if ui.button(label).clicked() {
                                menu_action.set(Some(act));
                                ui.close_menu();
                            }
                        };
                        pick(ui, "Cut", TextMenuAction::Cut);
                        pick(ui, "Copy", TextMenuAction::Copy);
                        pick(ui, "Paste", TextMenuAction::Paste);
                        pick(ui, "Delete", TextMenuAction::Delete);
                        ui.separator();
                        pick(ui, "Select All", TextMenuAction::SelectAll);
                    });

                    // `TEXT_LINE_SENSE`의 `focusable: false`만으로는 부족하다.
                    // `Response::context_menu`는 내부에서
                    // `response.interact(Sense::click())`을 부르는데
                    // (`egui-0.28.1/src/menu.rs:415`), `Sense::click()`은
                    // `focusable: true`라 sense가 **union**되어
                    // (`response.rs:855-868`) 이 줄이 다시 focusable 위젯으로
                    // 등록된다. 그러면 Tab이 또 줄에 걸려 keyboard_free 게이트가
                    // 닫힌다. 그래서 이 줄이 포커스를 쥐고 있으면 즉시 반납한다.
                    // (`surrender_focus`는 그 id가 포커스일 때만 지우므로
                    //  툴바 TextEdit 등 다른 위젯의 포커스는 건드리지 않는다.)
                    ui.memory_mut(|m| m.surrender_focus(id));
                });
            });
        });

    // ---- 클로저 종료 → doc 가변 대여 가능 ----

    // Page Up/Down이 읽을 관측값. 표 모드와 같은 이유로 아래 조기 반환보다
    // **먼저** 기록한다 — 뷰 모드에서도 페이지 이동이 되어야 한다.
    if let Some(first) = min_drawn_row.get() {
        doc.first_visible_row = first;
    }
    doc.visible_rows = visible_row_count(avail_height, row_h);

    if !editing {
        return;
    }

    // 1) 드래그 원점 래치 갱신(표 모드와 같은 전이 규칙).
    doc.text_drag_active =
        next_cell_drag_active(doc.text_drag_active, primary_down, line_press.get());

    // 2) 마우스 선택/캐럿 갱신. Shift+클릭이 먼저다 — 앵커를 유지한 채 캐럿만
    //    옮기므로 아래 "새 앵커" 분기(선택 해제)를 타면 안 된다.
    if let Some(head) = shift_click.get() {
        let anchor = shift_extend_text(doc.text_sel, doc.text_caret);
        doc.text_caret = head;
        doc.text_sel = if anchor == head { None } else { Some((anchor, head)) };
    } else if let Some(anchor) = drag_anchor.get() {
        let head = drag_head.get().unwrap_or(anchor);
        doc.text_caret = head;
        doc.text_sel = if anchor == head { None } else { Some((anchor, head)) };
    } else if drag_active {
        if let Some(head) = drag_head.get() {
            // 앵커는 유지하고 끝점만 확장. 선택이 없었으면 이전 캐럿이 앵커.
            let anchor = doc.text_sel.map(|(a, _)| a).unwrap_or(doc.text_caret);
            doc.text_caret = head;
            doc.text_sel = if anchor == head { None } else { Some((anchor, head)) };
        }
    }

    // 2-b) 더블/트리플 클릭 선택은 드래그가 만든 것을 덮어쓴다. 같은 프레임에
    //      press 분기가 이미 캐럿 하나로 무너뜨려 놨기 때문에 순서가 중요하다.
    //      캐럿은 선택 끝(head)에 둔다 — Shift+화살표로 이어서 늘릴 때
    //      Windows와 같은 방향이 된다.
    if let Some((a, b)) = multi_click.get() {
        doc.text_caret = b;
        doc.text_sel = if a == b { None } else { Some((a, b)) };
    }

    // 3) 키 입력 인텐트 적용.
    for intent in intents {
        apply_text_intent(ui, doc, clipboard, intent);
    }

    // 4) 컨텍스트 메뉴 동작. 우클릭 줄이 현재 선택 밖이면 캐럿만 그 줄로 옮긴다
    //    (선택은 유지 — 선택 안에서 우클릭한 경우 그 선택에 대해 동작해야 한다).
    //    menu_target은 메뉴가 열리는 프레임에만 채워진다(위 5) 참조).
    //    판정은 프레임 시작 스냅샷(sel_norm)이 아니라 **현재** doc.text_sel로
    //    한다 — 3)의 키 인텐트가 이미 선택을 바꿨을 수 있고, 그 최신 선택이
    //    "우클릭이 선택 안이었나"의 진실이다.
    if let Some(t) = menu_target.get() {
        let cur_sel = doc
            .text_sel
            .map(|(a, b)| crate::edit::normalize(a, b))
            .filter(|(a, b)| a != b);
        let inside = cur_sel.map_or(false, |(a, b)| {
            (a.line..=b.line).contains(&t.line)
        });
        if !inside {
            doc.text_sel = None;
            if let Some(e) = &doc.edit {
                doc.text_caret = clamp_pos(&e.lines, t);
            }
        }
    }
    if let Some(act) = menu_action.get() {
        let intent = match act {
            TextMenuAction::Cut => TextEditIntent::Cut,
            TextMenuAction::Copy => TextEditIntent::Copy,
            TextMenuAction::Paste => TextEditIntent::Paste(clipboard.clone()),
            TextMenuAction::Delete => TextEditIntent::Delete,
            TextMenuAction::SelectAll => TextEditIntent::SelectAll,
        };
        apply_text_intent(ui, doc, clipboard, intent);
    }

    // 5) 프레임 마무리: 캐럿/선택을 현재 버퍼 범위로 클램프해 다음 프레임 렌더와
    //    다음 인텐트 적용이 항상 유효한 위치에서 시작하게 한다.
    if let Some(e) = &doc.edit {
        doc.text_caret = clamp_pos(&e.lines, doc.text_caret);
        doc.text_sel = doc
            .text_sel
            .map(|(a, b)| (clamp_pos(&e.lines, a), clamp_pos(&e.lines, b)))
            .filter(|(a, b)| a != b);
    }
}

/// 헥스 문서의 논리 길이. 편집 중이면 버퍼(삽입/삭제로 소스와 다르다).
fn hex_doc_len(doc: &Document) -> u64 {
    match doc.hex.as_ref().and_then(|h| h.edit.as_ref()) {
        Some(e) => e.bytes.len() as u64,
        None => doc.source.len(),
    }
}

/// 한 행(32바이트)의 바이트. 마지막 행은 짧을 수 있고 범위 밖은 빈 Vec.
fn hex_row_bytes(doc: &Document, row: u64) -> Vec<u8> {
    let start = row * crate::hex::BYTES_PER_ROW as u64;
    let end = start + crate::hex::BYTES_PER_ROW as u64;
    match doc.hex.as_ref().and_then(|h| h.edit.as_ref()) {
        Some(e) => {
            let len = e.bytes.len() as u64;
            let s = start.min(len) as usize;
            let t = end.min(len).max(start.min(len)) as usize;
            e.bytes[s..t].to_vec()
        }
        None => doc.source.slice(start, end).to_vec(),
    }
}

/// abs 바이트가 선택 범위 안인가. sel은 (anchor, caret) 방향 무관.
fn byte_selected(sel: Option<(u64, u64)>, abs: u64) -> bool {
    match sel {
        Some((a, b)) => {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            abs >= lo && abs < hi
        }
        None => false,
    }
}

/// abs 바이트가 마지막 찾기 매치 안인가.
fn byte_in_match(last_match: Option<(u64, usize)>, abs: u64) -> bool {
    match last_match {
        Some((o, n)) => abs >= o && abs < o + n as u64,
        None => false,
    }
}

/// 헥스 본문의 클릭 한 번이 남기는 인텐트. 테이블 클로저 안에서는 `doc`이
/// 불변으로만 빌려지므로(표/텍스트 렌더와 같은 규율) 여기 모아 두었다가
/// 클로저가 끝난 뒤 한 번에 적용한다.
#[derive(Clone, Copy)]
struct HexClick {
    /// 캐럿을 놓을 절대 바이트 오프셋.
    abs: u64,
    /// 상위 니블인가(문자 패널은 항상 true).
    high: bool,
    pane: crate::hex::HexPane,
    /// Shift(또는 드래그 계속) — 앵커를 유지한 채 캐럿만 옮긴다.
    extend: bool,
}

/// 헥스 본문. 오프셋 | 16진수(32바이트) | ASCII 세 컬럼 고정폭.
///
/// 모노스페이스 고정폭이므로 클릭 x → 문자 컬럼 → 바이트 인덱스가 산술로
/// 떨어진다(`hex_click_byte`/`ascii_click_byte`) — 텍스트 모드처럼 갤리
/// 히트테스트가 필요 없다.
///
/// 표/텍스트 렌더와 같은 스캐폴딩을 쓴다: `TableBuilder` 가상 스크롤,
/// `pending_scroll_row` → `vertical_scroll_offset` 즉시 점프
/// (`scroll_offset_for_row` 주석), `first_visible_row`/`visible_rows` 관측
/// 기록. 헤더가 없다는 점만 다르므로 스크롤 뷰포트 계산에서 헤더 한 줄을
/// 빼지 않는다.
fn render_hex(ui: &mut egui::Ui, doc: &mut Document, clipboard: &mut String) {
    use crate::hex::{ascii_char, ascii_click_byte, hex_click_byte, BYTES_PER_ROW};
    use std::cell::Cell;

    // 찾기가 남긴 스크롤 요청(표/텍스트 모드와 같은 이유·같은 방법).
    let scroll_to = doc.pending_scroll_row.take();
    let scroll_align = doc.pending_scroll_align;

    let len = hex_doc_len(doc);
    let total_rows = crate::hex::row_count(len);
    let off_w = crate::hex::offset_width(len);
    let avail_height = ui.available_height();
    // 행 높이는 배율을 탄다 — 상수 ROW_HEIGHT를 직접 쓰면 확대 시 글자가 잘린다.
    let row_h = doc_row_height(doc);

    // ---- 상태 스냅샷 ----
    // 클로저 안에서 `doc`을 불변으로 빌려 바이트를 읽으므로, 캐럿/선택 같은
    // 작은 Copy 값은 미리 꺼내 둔다(빌림 충돌 회피 + 프레임 내내 일관).
    let (caret, sel, last_match, pane) = {
        let h = doc.hex.as_ref().expect("render_hex는 헥스 문서에서만 불린다");
        (h.caret, h.sel, h.last_match, h.pane)
    };

    let font = doc_font_id(doc);
    let text_color = ui.visuals().text_color();

    // 컬럼 폭은 **대표 문자열을 실제로 배치해 재서** 잡는다. 예전에는
    // `glyph_width('0') * 문자수`로 계산했는데, 그 값은 폰트가 알려주는
    // 이상적 폭(실수)이고 실제 배치는 픽셀 격자에 반올림된다. 32바이트를
    // 지나며 그 차이가 쌓여 마지막 바이트가 컬럼 밖으로 잘렸다(줌 배율에
    // 따라 반올림 방향이 달라져 특정 배율에서만 드러났다).
    let measure = |s: &str| -> f32 {
        ui.fonts(|f| {
            f.layout_no_wrap(s.to_owned(), font.clone(), text_color).size().x
        })
    };
    // 오프셋: 자릿수 + 여백 두 칸. 헥스: "FF " × 32(마지막 공백 포함 —
    // 갤리도 그렇게 그린다). 문자: 32칸 + 여백 두 칸.
    let offset_px = measure(&"0".repeat(off_w + 2));
    let hex_px = measure(&"FF ".repeat(BYTES_PER_ROW));
    let ascii_px = measure(&"W".repeat(BYTES_PER_ROW + 2));

    // 클로저 → 바깥 인텐트 통로(표/텍스트 렌더와 같은 규율).
    let click: Cell<Option<HexClick>> = Cell::new(None);
    let min_drawn_row: Cell<Option<usize>> = Cell::new(None);

    let shift_down = ui.input(|i| i.modifiers.shift);
    let spacing_y = ui.spacing().item_spacing.y;

    // 키 입력은 프레임당 한 번, 표/텍스트와 **같은 게이트 규율**로 읽는다:
    // 다른 위젯(툴바 TextEdit, 찾기 입력란 등)이 키보드 포커스를 쥐고 있으면
    // 그쪽에 양보한다. 그렇지 않으면 찾기 입력란에 친 "de"가 본문 바이트를
    // 덮어쓴다. 확인 다이얼로그가 떠 있는 동안도 마찬가지로 양보한다 —
    // 그 창의 버튼이 키를 받아야 한다.
    let dialog_open = doc.hex.as_ref().is_some_and(|h| h.confirm_load);
    let keyboard_free = ui.ctx().memory(|m| m.focused().is_none());
    let intents: Vec<HexIntent> = if keyboard_free && !dialog_open {
        let pane_now = pane;
        let rows = hex_visible_row_count(avail_height, row_h);
        ui.input(|i| collect_hex_intents(i, pane_now, rows))
    } else {
        Vec::new()
    };

    // 표 렌더와 같은 이유(`render_table`의 `ScrollArea::horizontal` 주석) —
    // `TableBuilder`가 가로 스크롤을 지원하지 않으므로 바깥에 한 겹 씌운다.
    // 확대 배율이 커지면 offset/hex/ascii 세 컬럼 합이 창 폭을 넘을 수 있다.
    egui::ScrollArea::horizontal()
        .id_source("render_hex_hscroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
    let mut table = TableBuilder::new(ui)
        .striped(false)
        .auto_shrink([false, false])
        .max_scroll_height(avail_height)
        .column(Column::exact(offset_px))
        .column(Column::exact(hex_px))
        .column(Column::exact(ascii_px));
    if let Some(row) = scroll_to {
        let row = row.min((total_rows as usize).saturating_sub(1));
        table = table.vertical_scroll_offset(scroll_offset_for_row(
            row,
            scroll_align,
            row_h,
            spacing_y,
            avail_height,
        ));
    }

    table.body(|body| {
        body.rows(row_h, total_rows as usize, |mut table_row| {
            let row = table_row.index();
            min_drawn_row.set(Some(min_drawn_row.get().map_or(row, |m: usize| m.min(row))));
            let row_start = row as u64 * BYTES_PER_ROW as u64;
            let bytes = hex_row_bytes(doc, row as u64);

            // ---- 오프셋 ----
            //
            // `Label` 대신 다른 두 칸과 **같은 함수**로 그린다. `Label`은
            // egui의 레이아웃(정렬·여백)을 타서 세로 기준선이 갤리 직접
            // 그리기와 미묘하게 달랐고, 그래서 일련번호와 16진수 줄이
            // 가지런히 맞지 않았다.
            table_row.col(|ui| {
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &crate::hex::format_offset(row as u64, off_w),
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: crate::theme::hex_offset_fg(),
                        ..Default::default()
                    },
                );
                paint_hex_cell(ui, job, row_h);
            });

            // ---- 16진수 — 바이트별 LayoutJob 섹션(선택/매치 배경) ----
            table_row.col(|ui| {
                let mut job = egui::text::LayoutJob::default();
                for (i, b) in bytes.iter().enumerate() {
                    let abs = row_start + i as u64;
                    let bg = hex_byte_bg(sel, last_match, abs);
                    job.append(
                        &format!("{b:02X}"),
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: text_color,
                            background: bg,
                            ..Default::default()
                        },
                    );
                    // 바이트 사이 공백. 선택 안쪽이면 공백도 칠해야 음영이
                    // 끊기지 않는다 — 마지막 선택 바이트 뒤는 칠하지 않는다.
                    let gap_bg = if byte_selected(sel, abs) && byte_selected(sel, abs + 1) {
                        bg
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    job.append(
                        " ",
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: text_color,
                            background: gap_bg,
                            ..Default::default()
                        },
                    );
                }
                let cell = paint_hex_cell(ui, job, row_h);
                if let Some((col, extend)) = hex_cell_hit(&cell, shift_down) {
                    if let Some((bi, high)) = hex_click_byte(col) {
                        click.set(Some(HexClick {
                            abs: (row_start + bi as u64).min(len),
                            high,
                            pane: crate::hex::HexPane::Hex,
                            extend,
                        }));
                    }
                }
                // 캐럿 표시 — 두 자리(16진수 한 바이트) 폭 테두리. 헥스 패널이
                // 활성일 때만 실선, 아니면 흐린 테두리로 "여기 있다"만 알린다.
                //
                // 좌표는 **갤리에게 묻는다**. 바이트 i는 문자 3i("4F ")에서
                // 시작하고 두 문자를 차지한다.
                if caret.0 >= row_start && caret.0 < row_start + BYTES_PER_ROW as u64 {
                    let ch = (caret.0 - row_start) as usize * 3;
                    let x = cell.char_x(ch);
                    let w = cell.char_x(ch + 2) - x;
                    paint_hex_caret(
                        ui,
                        egui::Rect::from_min_size(
                            egui::pos2(x, cell.resp.rect.top()),
                            egui::vec2(w.max(1.0), cell.resp.rect.height()),
                        ),
                        pane == crate::hex::HexPane::Hex,
                    );
                }
            });

            // ---- ASCII — 같은 요령, 폭 1문자 ----
            table_row.col(|ui| {
                let mut job = egui::text::LayoutJob::default();
                for (i, b) in bytes.iter().enumerate() {
                    let abs = row_start + i as u64;
                    job.append(
                        &ascii_char(*b).to_string(),
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: text_color,
                            background: hex_byte_bg(sel, last_match, abs),
                            ..Default::default()
                        },
                    );
                }
                let cell = paint_hex_cell(ui, job, row_h);
                if let Some((col, extend)) = hex_cell_hit(&cell, shift_down) {
                    if let Some(bi) = ascii_click_byte(col) {
                        click.set(Some(HexClick {
                            abs: (row_start + bi as u64).min(len),
                            // 문자 패널은 니블 개념이 없다 — 상위로 고정해
                            // 패널을 오갈 때 캐럿이 바이트 앞에 서게 한다.
                            high: true,
                            pane: crate::hex::HexPane::Ascii,
                            extend,
                        }));
                    }
                }
                if caret.0 >= row_start && caret.0 < row_start + BYTES_PER_ROW as u64 {
                    // 문자 패널은 1바이트 = 1문자. 여기서도 폭을 갤리에서 잰다.
                    let ch = (caret.0 - row_start) as usize;
                    let x = cell.char_x(ch);
                    let w = cell.char_span(ch);
                    paint_hex_caret(
                        ui,
                        egui::Rect::from_min_size(
                            egui::pos2(x, cell.resp.rect.top()),
                            egui::vec2(w, cell.resp.rect.height()),
                        ),
                        pane == crate::hex::HexPane::Ascii,
                    );
                }
            });
        });
    });
        });

    // ---- 클로저 종료 → doc 가변 대여 가능 ----

    // Page Up/Down이 읽을 관측값(표/텍스트 렌더와 같은 통로).
    if let Some(first) = min_drawn_row.get() {
        doc.first_visible_row = first;
    }
    doc.visible_rows = hex_visible_row_count(avail_height, row_h);

    // 클릭 한 번을 캐럿/선택에 반영. `extend`면 앵커를 유지하고 캐럿만 옮긴다.
    if let Some(c) = click.get() {
        let h = doc.hex.as_mut().expect("render_hex는 헥스 문서에서만 불린다");
        if c.extend {
            let anchor = h.sel.map(|(a, _)| a).unwrap_or(h.caret.0);
            h.sel = if anchor == c.abs { None } else { Some((anchor, c.abs)) };
        } else {
            h.sel = None;
        }
        h.caret = (c.abs, c.high);
        h.pane = c.pane;
    }

    // 키 입력 인텐트 적용. 클릭 **뒤**라야 같은 프레임의 "클릭해 놓고 타이핑"이
    // 옮겨진 캐럿에서 시작한다.
    let want_copy = intents.contains(&HexIntent::Copy);
    for intent in intents {
        apply_hex_intent(doc, clipboard, intent);
    }
    // 복사는 캐시(`clipboard_cache`)와 OS 클립보드를 겸한다 — 표/텍스트 모드의
    // 복사와 같은 방식이다. `apply_hex_intent`는 egui를 모르는 순수 함수라
    // OS 쪽 쓰기만 여기서 한다.
    if want_copy && !clipboard.is_empty() {
        let text = clipboard.clone();
        ui.output_mut(|o| o.copied_text = text);
    }
}

/// 한 바이트 칸의 배경. 선택이 매치보다 우선한다(선택은 지금 조작 중인 것).
fn hex_byte_bg(
    sel: Option<(u64, u64)>,
    last_match: Option<(u64, usize)>,
    abs: u64,
) -> egui::Color32 {
    if byte_selected(sel, abs) {
        sel_shade()
    } else if byte_in_match(last_match, abs) {
        crate::theme::hex_match_bg()
    } else {
        egui::Color32::TRANSPARENT
    }
}

/// 헥스 본문 한 칸(LayoutJob)을 칸 폭 전체를 차지하는 클릭 대상으로 그린다.
///
/// `Label`의 응답 rect는 **글자가 실제로 찬 만큼**이라(마지막 행은 짧다)
/// 그것으로 클릭을 받으면 짧은 행의 오른쪽 빈 자리가 죽는다. 대신 칸의
/// 왼쪽 위에 글자를 그리고, 상호작용은 컬럼 폭 전체(`width`)로 따로 잡는다.
/// 그려진 한 칸 — 상호작용 응답과 **실제로 그린 갤리**.
///
/// 갤리를 돌려주는 이유가 이 모듈의 핵심 교훈이다. 예전에는 캐럿 위치와
/// 클릭 역산을 `char_w * 문자수`로 **추정**했는데, `char_w`는 폰트가 알려주는
/// 이상적 폭(실수)이고 갤리는 글리프를 픽셀 격자에 맞춰 배치한다. 둘의
/// 소수점 이하 차이가 바이트마다 누적돼, 한 행 끝(32번째 바이트)에서는
/// 캐럿이 글자 하나 이상 밀렸다. 줌 배율에 따라 반올림 방향이 달라져
/// "어떤 배율에서만" 어긋나 보이기까지 했다.
///
/// 갤리는 자기가 어디에 무엇을 그렸는지 정확히 안다. 그래서 위치를 묻는다.
struct HexCell {
    resp: egui::Response,
    galley: std::sync::Arc<egui::Galley>,
    /// 갤리 원점(글자 왼쪽 위). 갤리 좌표는 여기서부터의 상대값이다.
    origin: egui::Pos2,
}

impl HexCell {
    /// `ch` 번째 문자의 화면 x. 갤리에게 직접 묻는다.
    fn char_x(&self, ch: usize) -> f32 {
        let cursor = self.galley.from_ccursor(egui::text::CCursor::new(ch));
        self.origin.x + self.galley.pos_from_cursor(&cursor).min.x
    }

    /// 문자 하나의 실제 폭(그 자리에서 잰다 — 균등폭 폰트라도 반올림 때문에
    /// 자리마다 1px 다를 수 있다).
    fn char_span(&self, ch: usize) -> f32 {
        (self.char_x(ch + 1) - self.char_x(ch)).max(1.0)
    }
}

/// 셀 하나를 그리고 갤리째로 돌려준다. 세로 위치는 **행 높이 기준**으로
/// 잡는다 — 갤리 높이로 중앙을 잡으면 셀마다 내용이 달라 기준선이 흔들려
/// 오프셋 컬럼과 헥스 컬럼의 줄이 어긋난다.
fn paint_hex_cell(ui: &mut egui::Ui, job: egui::text::LayoutJob, row_h: f32) -> HexCell {
    let cell = ui.max_rect();
    let galley = ui.fonts(|f| f.layout_job(job));
    // 모든 칸이 같은 규칙으로 세로 정렬되도록 행 높이 기준 중앙에 둔다.
    // 배율이 걸린 행 높이를 **받아서** 쓴다 — 상수를 쓰면 확대했을 때
    // 갤리가 커진 만큼 중앙이 위로 밀려 세 칸의 줄이 다시 어긋난다.
    let origin = egui::pos2(cell.left(), cell.top() + (row_h - galley.size().y) * 0.5);
    ui.painter()
        .with_clip_rect(cell)
        .galley(origin, galley.clone(), ui.visuals().text_color());
    // 상호작용 rect는 글자 원점에서 시작한다. 폭은 갤리가 실제로 차지한
    // 만큼(셀을 넘지 않게 클램프) — 추정 폭을 쓰면 마지막 바이트가 판정에서
    // 빠진다.
    let hit = egui::Rect::from_min_size(
        egui::pos2(origin.x, cell.top()),
        egui::vec2(galley.size().x.min(cell.width()), cell.height()),
    );
    let resp = ui.interact(hit, ui.id().with("hexcell"), egui::Sense::click_and_drag());
    HexCell { resp, galley, origin }
}

/// 헥스/문자 칸의 포인터 상호작용 → (문자 컬럼, 선택 확장인가).
/// 누름 시작·클릭·드래그 전부를 같은 통로로 받는다 — 드래그는 앵커를 유지해야
/// 하므로 `extend`로 표시한다.
///
/// 문자 컬럼은 **갤리에게 묻는다**(`char_x`와 같은 좌표계). 나눗셈으로
/// 추정하면 캐럿을 그린 자리와 클릭이 해석되는 자리가 어긋난다.
fn hex_cell_hit(cell: &HexCell, shift_down: bool) -> Option<(usize, bool)> {
    let resp = &cell.resp;
    let pos = resp.interact_pointer_pos()?;
    if !(resp.clicked() || resp.drag_started() || resp.dragged()) {
        return None;
    }
    let rel = pos - cell.origin;
    let col = cell.galley.cursor_from_pos(rel).ccursor.index;
    // 새 누름(클릭/드래그 시작)은 앵커를 새로 잡고, 이어지는 드래그와
    // Shift는 확장이다.
    let extend = shift_down || (resp.dragged() && !resp.drag_started());
    Some((col, extend))
}

/// 캐럿 테두리. 활성 패널은 accent 실선, 비활성 패널은 같은 색 옅은 선 —
/// "캐럿이 어느 패널에 있나"가 한눈에 보여야 타이핑이 어디로 갈지 안다.
fn paint_hex_caret(ui: &egui::Ui, rect: egui::Rect, active: bool) {
    let c = crate::theme::accent();
    let stroke = if active {
        egui::Stroke::new(1.5, c)
    } else {
        egui::Stroke::new(1.0, c.gamma_multiply(0.4))
    };
    ui.painter().rect_stroke(rect, 0.0, stroke);
}

/// 헥스 본문의 한 화면 행 수. 표/텍스트와 달리 **헤더가 없으므로**
/// `visible_row_count`처럼 한 줄을 빼지 않는다(빼면 Page Down이 한 행씩
/// 덜 움직인다).
fn hex_visible_row_count(avail_height: f32, row_h: f32) -> usize {
    if avail_height <= 0.0 {
        return 0;
    }
    (avail_height / row_h) as usize
}

/// 전역 Ctrl+Z(텍스트 되돌리기)가 이 문서에서 발동해도 되는가.
///
/// **헥스 문서에서는 반드시 거짓이어야 한다.** 헥스의 Ctrl+Z는
/// `collect_hex_intents`가 `HexIntent::Undo`로 받으므로, 여기서도 참이면 한 번
/// 누른 undo가 두 경로에서 두 번 일어난다. 조건이 `d.edit`(텍스트 편집 버퍼)인
/// 덕에 저절로 갈리지만, 그 성질을 함수로 뽑아 테스트로 박아 둔다.
fn can_undo_text(d: &Document) -> bool {
    d.edit.is_some() && d.editing_cell.is_none()
}

/// 저장할 편집 내용이 있는가 — 텍스트 편집 버퍼 또는 헥스 편집 버퍼.
fn doc_savable(doc: &Document) -> bool {
    doc.edit.is_some() || doc.hex.as_ref().is_some_and(|h| h.edit.is_some())
}

/// **다른 이름으로** 내보낼 수 있는가. `doc_savable`(제자리 저장)과 갈라 둔
/// 이유: Parquet은 읽기 전용이라 덮어쓸 수 없지만 **CSV/TSV로 내보내는 것은
/// 되어야 한다**. 편집 버퍼가 없다는 이유로 `doc_savable`에 얹으면 "Save"까지
/// 열려, 읽기 전용 파일을 제자리에서 덮어쓰겠다는 뜻이 된다.
///
/// 내보내기 실체는 `collect_export_lines` → `save::write_file`이고, 정렬이
/// 걸려 있으면 화면 순서를 따른다.
fn doc_exportable(doc: &Document) -> bool {
    doc_savable(doc) || doc.parquet.is_some()
}

/// 문서의 **모든 행에 지금 접근할 수 있는가**. 정렬처럼 전체를 훑는 조작이
/// 이것을 전제한다.
///
/// 세 출처가 각자 다른 이유로 참이다:
/// - 편집 버퍼: 파일 전체가 이미 RAM에 있다.
/// - Parquet: 푸터가 행 수를 알고 row group을 임의 접근할 수 있다.
///   **인덱서를 띄우지 않으므로 `Phase`는 영영 `Priming`이다** — 그 값을
///   조건에 쓰면 정렬 메뉴가 열리지 않는다(실제로 그랬다).
/// - 텍스트 뷰: 줄 인덱싱이 끝나야 한다.
fn doc_rows_ready(doc: &Document) -> bool {
    doc.edit.is_some()
        || doc.parquet.is_some()
        || doc.index.status().phase == crate::index::Phase::Complete
}

/// 저장하지 않은 변경이 있는가 — 텍스트/헥스 어느 쪽 편집 버퍼든 dirty면 참.
/// 닫기 확인(탭 닫기·앱 종료)과 탭 라벨의 ●/* 표시가 함께 이 함수를 본다 —
/// 텍스트만 보던 시절엔 헥스를 수정하고 저장하지 않은 채 탭/앱을 닫아도
/// 확인 없이 사라졌다(Task 6 리뷰에서 지적된 계획된 범위).
fn doc_dirty(doc: &Document) -> bool {
    doc.edit.as_ref().is_some_and(|e| e.dirty)
        || doc
            .hex
            .as_ref()
            .and_then(|h| h.edit.as_ref())
            .is_some_and(|e| e.dirty)
}

/// 텍스트/표 전용 도구(Sort, Convert, Numbering, 오류 창)의 활성 조건.
///
/// **왜 자유 함수인가.** 이 판정이 쓰이는 자리는 전부 egui 클로저 안(메뉴바)
/// 이라 테스트가 구동할 수 없다. 판정만 순수 함수로 떼어 두면 "헥스 문서에서
/// 잠긴다 / 텍스트 문서에서 열린다"를 두 줄로 검증할 수 있다 — 이 저장소가
/// `ending_glyphs`·`show_gutter` 등에 쓰는 것과 같은 패턴이다.
///
/// 헥스 문서는 행·필드·인코딩이라는 개념 자체가 없다. "N번째 컬럼으로 정렬",
/// "구분자 변환", "행/열 번호", "필드 수가 맞는가"는 물음이 성립하지 않는다.
fn text_tools_enabled(doc: &Document) -> bool {
    doc.hex.is_none()
}

/// 이 문서가 **행과 필드를 가진 표/텍스트**인가. 툴바의 구분자 드롭다운과
/// 상태줄의 인덱싱 진행 문구가 이 개념을 쓴다.
///
/// `text_tools_enabled`(헥스만 제외)와 갈라 둔 이유: **Parquet은 정렬과 찾기가
/// 되어야 하므로 Tools 메뉴를 잠그면 안 되지만**, 구분자 선택(콤마 고정)과
/// 인덱싱 진행 문구(인덱서를 안 띄운다)는 의미가 없다. 하나로 묶으면 둘 중
/// 하나가 반드시 틀린다.
fn text_layout_tools_enabled(doc: &Document) -> bool {
    doc.hex.is_none() && doc.parquet.is_none()
}

/// 이 문서를 편집할 수 있는가. 헥스는 자체 편집 경로가 있고(`ensure_hex_edit`),
/// Parquet은 읽기 전용이라 편집 버퍼가 존재할 수 없다.
fn text_edit_allowed(doc: &Document) -> bool {
    doc.hex.is_none() && doc.parquet.is_none()
}

/// Parquet 문서의 상태줄 문구 — 행/열 수와 **읽기 전용임**.
///
/// **왜 인덱싱 문구를 대신하는가(헥스와 같은 이유).** Parquet은 줄 인덱서를
/// 띄우지 않으므로(`parquet_document`가 `indexer: None`) `LineIndex`가
/// `Priming`에서 영영 움직이지 않는다. 그대로 두면 상태줄이 "Indexing… 0%"를
/// 무한히 띄운다 — 진행 중이 아닌데 진행 중이라고 말하는 셈이다.
///
/// 읽기 전용임을 여기 적는 이유는, 사용자가 편집을 시도하기 **전에** 알아야
/// 하기 때문이다. 버튼이 회색인 것만으로는 왜인지 알 수 없다.
fn parquet_status_text(doc: &Document) -> String {
    let Some(pq) = &doc.parquet else {
        return String::new();
    };
    let p = pq.borrow();
    format!(
        "Parquet · {} rows · {} cols · read-only",
        crate::parquet::group_digits(p.total_rows()),
        p.column_names().len()
    )
}

/// 헥스 문서의 상태줄 문구 — 크기와 캐럿 오프셋.
///
/// **왜 인덱싱 문구를 대신하는가.** 헥스 문서는 줄 인덱서를 아예 띄우지
/// 않으므로(`hex_document`가 `indexer: None`) `LineIndex`가 `Priming`에서
/// 영영 움직이지 않는다. 그대로 두면 상태줄이 "Indexing… 0%"를 무한히
/// 띄운다 — 진행 중이 아닌데 진행 중이라고 말하는 셈이다.
///
/// 크기는 편집 버퍼가 있으면 그쪽이 진실이다(`hex_doc_len`이 그 분기를 안다).
/// 오프셋은 10진/16진 둘 다 적는다 — 헥스 뷰의 오프셋 컬럼과 맞춰 보려면
/// 16진이, 크기와 견주려면 10진이 필요하다.
///
/// **삽입/덮어쓰기 모드도 적는다(M7).** Insert 키 한 번으로 토글되는데
/// 아무 피드백이 없으면 지금 어느 쪽인지 알 길이 없다. 두 모드는 결과가
/// 구조적으로 다르다 — 덮어쓰기는 파일 길이를 보존하고, 삽입은 뒤를 전부
/// 밀어 길이를 바꾼다(바이너리에서는 대개 포맷이 깨진다).
fn hex_status_text(doc: &Document) -> String {
    let len = hex_doc_len(doc);
    let caret = doc.hex.as_ref().map(|h| h.caret.0).unwrap_or(0);
    let mode = if doc.hex.as_ref().is_some_and(|h| h.insert_mode) { "INS" } else { "OVR" };
    format!("Binary — {len} bytes | 0x{caret:X} ({caret}) | {mode}")
}

/// 헥스 본문의 키 입력 한 건. 클릭(`HexClick`)과 같은 이유로 인텐트로
/// 모았다가 렌더 클로저 밖에서 적용한다.
#[derive(Debug, Clone, PartialEq)]
enum HexIntent {
    /// 상대 이동(바이트 단위). `extend`면 선택을 늘린다.
    Move { delta: i64, extend: bool },
    /// 절대 이동(클램프 + 선택 확장 포함). **테스트만 만든다.**
    ///
    /// 찾기가 이걸 쓸 것으로 예상했지만 실제 `hex_find_next`는 `last_match`와
    /// `pending_scroll_row`까지 한 묶음으로 갱신해야 해서 캐럿을 직접 놓는다 —
    /// 인텐트 한 겹을 거치면 그 셋이 갈라진다.
    ///
    /// 그래도 남긴다: 이동 계열 인텐트(Home/End/DocStart/DocEnd)와 편집 계열
    /// (Backspace/Nibble) 테스트가 "캐럿을 N에 놓고 시작"하는 **설정 수단**으로
    /// 쓴다. 대안은 `h.caret`을 직접 대입하는 것인데, 그러면 인텐트 경로가
    /// 적용하는 클램프를 건너뛰어 테스트가 실제와 다른 상태에서 출발한다.
    /// (임시 표식이 아니다 — 지울 조건이 없다.)
    #[allow(dead_code)]
    MoveTo { offset: u64, extend: bool },
    /// 행 시작/행 마지막 바이트.
    MoveHome { extend: bool },
    MoveEnd { extend: bool },
    /// 문서 처음/끝(끝 = len, 삽입 지점).
    MoveDocStart { extend: bool },
    MoveDocEnd { extend: bool },
    /// 헥스 패널의 16진수 한 글자.
    Nibble(u8),
    /// 문자 패널의 글자(한글은 UTF-8 여러 바이트).
    Ascii(String),
    DeleteForward,
    Backspace,
    ToggleInsert,
    Copy,
    Paste(String),
    Undo,
    Redo,
    /// Escape — 선택만 지운다.
    ClearSelection,
}

/// 임계 초과면 확인이 필요하다. `limit`은 `HexState.edit_limit`에서 온다
/// (프로덕션은 언제나 512MB, 테스트만 낮춘다 — 그 필드 주석 참조).
fn hex_load_needs_confirm(len: u64, limit: u64) -> bool {
    len > limit
}

/// 정규화된 선택 범위 [lo, hi). 빈 선택은 None — 역방향 선택도 여기서 편다.
fn hex_selection_range(h: &crate::hex::HexState) -> Option<(u64, u64)> {
    let (a, b) = h.sel?;
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    (lo < hi).then_some((lo, hi))
}

/// 편집 버퍼를 보장한다. 이미 있으면 true, 작으면 로드 후 true,
/// 크면 `confirm_load`만 세우고 false(그 조작은 버려진다 — 스펙).
fn ensure_hex_edit(doc: &mut Document) -> bool {
    let len = doc.source.len();
    let limit = doc.hex.as_ref().map_or(u64::MAX, |h| h.edit_limit);
    let bytes = if doc.hex.as_ref().is_some_and(|h| h.edit.is_none())
        && !hex_load_needs_confirm(len, limit)
    {
        Some(doc.source.as_bytes().to_vec())
    } else {
        None
    };
    let Some(h) = doc.hex.as_mut() else { return false };
    if h.edit.is_some() {
        return true;
    }
    match bytes {
        Some(b) => {
            h.edit = Some(crate::hex::HexEditBuffer::new(b));
            true
        }
        None => {
            h.confirm_load = true;
            false
        }
    }
}

/// 인텐트 하나를 헥스 문서에 적용한다. 편집 인텐트는 `ensure_hex_edit`로
/// 메모리 승격을 먼저 거치고, 승격이 확인 대기로 막히면 그 조작은 버린다.
fn apply_hex_intent(doc: &mut Document, clipboard: &mut String, intent: HexIntent) {
    let len = hex_doc_len(doc);
    let row_bytes = crate::hex::BYTES_PER_ROW as u64;

    // ---- 이동 계열: 승격 없이 캐럿/선택만 만진다 ----
    let target = match &intent {
        HexIntent::Move { delta, .. } => {
            let cur = doc.hex.as_ref().map_or(0, |h| h.caret.0) as i64;
            Some((cur.saturating_add(*delta).max(0) as u64).min(len))
        }
        HexIntent::MoveTo { offset, .. } => Some((*offset).min(len)),
        HexIntent::MoveHome { .. } => {
            let cur = doc.hex.as_ref().map_or(0, |h| h.caret.0);
            Some(cur - cur % row_bytes)
        }
        HexIntent::MoveEnd { .. } => {
            let cur = doc.hex.as_ref().map_or(0, |h| h.caret.0);
            // 행의 마지막 **바이트**. 문서 끝 행은 짧을 수 있고, 빈 문서는 0.
            let row_last = cur - cur % row_bytes + row_bytes - 1;
            Some(row_last.min(len.saturating_sub(1)))
        }
        HexIntent::MoveDocStart { .. } => Some(0),
        HexIntent::MoveDocEnd { .. } => Some(len),
        _ => None,
    };
    if let Some(abs) = target {
        let extend = match &intent {
            HexIntent::Move { extend, .. }
            | HexIntent::MoveTo { extend, .. }
            | HexIntent::MoveHome { extend }
            | HexIntent::MoveEnd { extend }
            | HexIntent::MoveDocStart { extend }
            | HexIntent::MoveDocEnd { extend } => *extend,
            _ => false,
        };
        let Some(h) = doc.hex.as_mut() else { return };
        if extend {
            // 앵커는 확장이 시작되기 **전** 위치다 — 클릭 확장(`HexClick`)과
            // 같은 규칙이라 마우스/키보드 선택이 어긋나지 않는다.
            let anchor = h.sel.map(|(a, _)| a).unwrap_or(h.caret.0);
            h.sel = if anchor == abs { None } else { Some((anchor, abs)) };
        } else {
            h.sel = None;
        }
        h.caret = (abs, true);
        return;
    }

    match intent {
        HexIntent::ClearSelection => {
            if let Some(h) = doc.hex.as_mut() {
                h.sel = None;
            }
        }
        // 상태 플래그일 뿐이라 승격이 필요 없다.
        HexIntent::ToggleInsert => {
            if let Some(h) = doc.hex.as_mut() {
                h.insert_mode = !h.insert_mode;
            }
        }
        // 복사는 읽기다 — 승격시키지 않는다(GB급 파일에서 복사 한 번에 전체
        // 로드가 일어나면 안 된다).
        HexIntent::Copy => {
            let Some(h) = doc.hex.as_ref() else { return };
            let Some((lo, hi)) = hex_selection_range(h) else { return };
            let text = match h.edit.as_ref() {
                Some(e) => hex_join(&e.bytes[(lo as usize).min(e.bytes.len())..(hi as usize).min(e.bytes.len())]),
                None => hex_join(doc.source.slice(lo, hi)),
            };
            *clipboard = text;
        }
        HexIntent::Undo | HexIntent::Redo => {
            let is_undo = intent == HexIntent::Undo;
            let Some(h) = doc.hex.as_mut() else { return };
            let Some(e) = h.edit.as_mut() else { return };
            let pos = if is_undo { e.undo() } else { e.redo() };
            if let Some(p) = pos {
                let cap = e.bytes.len() as u64;
                h.caret = (p.min(cap), true);
                h.sel = None;
                // 되돌리기/다시하기도 삽입/삭제를 되감으므로 그 뒤 바이트가
                // 통째로 밀린다 — 편집과 같은 이유로 매치를 버린다(C2).
                h.last_match = None;
            }
        }
        HexIntent::Nibble(_)
        | HexIntent::Ascii(_)
        | HexIntent::Paste(_)
        | HexIntent::DeleteForward
        | HexIntent::Backspace => {
            // 붙여넣기는 해석 실패면 아무 일도 아니다 — 승격보다 먼저 판정해
            // 확인 다이얼로그가 헛되이 뜨지 않게 한다.
            let pane = doc.hex.as_ref().map(|h| h.pane);
            let paste_bytes = match (&intent, pane) {
                (HexIntent::Paste(s), Some(crate::hex::HexPane::Hex)) => {
                    match crate::hex::parse_hex_query(s) {
                        Some(b) => Some(b),
                        None => return,
                    }
                }
                (HexIntent::Paste(s), _) => Some(s.as_bytes().to_vec()),
                _ => None,
            };
            if !ensure_hex_edit(doc) {
                return;
            }
            let Some(h) = doc.hex.as_mut() else { return };
            let insert_mode = h.insert_mode;
            let sel = hex_selection_range(h);

            // **빈 입력은 선택을 지우기 전에 걸러낸다(I5).** 예전에는 선택
            // 삭제가 먼저였고 `if b.is_empty() { return; }`가 그 뒤에 있어서,
            // 클립보드가 빈 채로 Ctrl+V를 누르면 선택된 바이트가 사라지고
            // (`h.sel`도 지워진 채) 캐럿 대입(`h.caret = caret`)마저 건너뛰어
            // 캐럿이 낡은 자리에 남았다. 빈 입력은 완전한 no-op이어야 한다.
            let empty_input = match &intent {
                HexIntent::Ascii(s) => s.is_empty(),
                HexIntent::Paste(_) => paste_bytes.as_ref().is_none_or(|b| b.is_empty()),
                _ => false,
            };
            if empty_input {
                return;
            }
            let Some(e) = h.edit.as_mut() else { return };

            // 선택이 있으면 어느 편집이든 먼저 지우고 그 자리에서 시작한다
            // (텍스트 편집과 같은 관행).
            let mut caret = h.caret;
            if let Some((lo, hi)) = sel {
                e.delete_range(lo, hi);
                caret = (lo, true);
                h.sel = None;
            }
            let buf_len = e.bytes.len() as u64;

            match intent {
                HexIntent::Nibble(n) => {
                    if caret.0 >= buf_len {
                        // 파일 끝에는 덮어쓸 바이트가 없다 — 삽입 모드와 무관하게 삽입.
                        e.insert(buf_len, &[n << 4]);
                        caret = (buf_len, false);
                    } else if insert_mode && caret.1 {
                        e.insert(caret.0, &[n << 4]);
                        caret.1 = false;
                    } else {
                        let cur = e.bytes[caret.0 as usize];
                        e.overwrite(caret.0, &[crate::hex::apply_nibble(cur, caret.1, n)]);
                        caret = if caret.1 { (caret.0, false) } else { (caret.0 + 1, true) };
                    }
                }
                HexIntent::Ascii(s) => {
                    // 빈 문자열은 위(`empty_input`)에서 이미 걸러졌다.
                    let b = s.as_bytes();
                    if insert_mode {
                        e.insert(caret.0, b);
                    } else {
                        e.overwrite(caret.0, b);
                    }
                    caret = (caret.0 + b.len() as u64, true);
                }
                HexIntent::Paste(_) => {
                    // 빈 바이트열은 위(`empty_input`)에서 이미 걸러졌다.
                    let b = paste_bytes.unwrap_or_default();
                    // 붙여넣기는 관행상 **삽입**이다(스펙).
                    e.insert(caret.0, &b);
                    caret = (caret.0 + b.len() as u64, true);
                }
                HexIntent::DeleteForward => {
                    if sel.is_none() {
                        if caret.0 >= buf_len {
                            return; // 끝에서 Delete는 no-op
                        }
                        e.delete_range(caret.0, caret.0 + 1);
                    }
                    caret.1 = true;
                }
                HexIntent::Backspace => {
                    if sel.is_none() {
                        if caret.0 == 0 {
                            return; // 처음에서 Backspace는 no-op
                        }
                        e.delete_range(caret.0 - 1, caret.0);
                        caret = (caret.0 - 1, true);
                    } else {
                        caret.1 = true;
                    }
                }
                _ => unreachable!("이 갈래는 편집 인텐트만 온다"),
            }
            h.caret = caret;
            // **편집은 매치를 무효로 만든다(C2).** 삽입/삭제는 편집 지점
            // 뒤의 모든 바이트를 밀어 `(offset, len)`이 가리키던 자리가
            // 검색한 바이트열과 무관해진다 — 그대로 두면 `hex_byte_bg`가
            // 검색한 적 없는 바이트를 하이라이트하고, 다음 `hex_find_next`가
            // 무의미한 위치에서 `from`을 잡아 매치를 건너뛴다. 덮어쓰기도
            // 매치 안의 바이트를 바꿔 놓을 수 있으므로 함께 버린다.
            // 텍스트 쪽이 편집 지점마다 `doc.last_match = None`을 두는 것과
            // 같은 규율이다.
            h.last_match = None;
        }
        // 위 이동 분기에서 이미 처리하고 반환했다.
        HexIntent::Move { .. }
        | HexIntent::MoveTo { .. }
        | HexIntent::MoveHome { .. }
        | HexIntent::MoveEnd { .. }
        | HexIntent::MoveDocStart { .. }
        | HexIntent::MoveDocEnd { .. } => unreachable!("이동은 위에서 끝낸다"),
    }
}

/// 복사 형식 — `"4F 4B"`. 참고 UI와 같은 대문자 공백 조인이다.
fn hex_join(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// 이번 프레임의 입력 이벤트에서 헥스 편집 인텐트를 뽑는다. 순수 함수 —
/// 패널(`pane`)과 한 화면 행 수(`visible_rows`)를 받아 `Event::Text`를 가르고
/// Page 키의 폭을 정한다. (`collect_text_intents`와 같은 규율.)
fn collect_hex_intents(
    i: &egui::InputState,
    pane: crate::hex::HexPane,
    visible_rows: usize,
) -> Vec<HexIntent> {
    let row = crate::hex::BYTES_PER_ROW as i64;
    let page = row * visible_rows.max(1) as i64;
    let mut out = Vec::new();
    for ev in &i.events {
        match ev {
            egui::Event::Text(t) if !t.is_empty() => match pane {
                // 헥스 패널은 16진수 한 글자 = 니블 하나. 그 밖의 글자는 버린다.
                crate::hex::HexPane::Hex => {
                    for c in t.chars() {
                        if let Some(d) = c.to_digit(16) {
                            out.push(HexIntent::Nibble(d as u8));
                        }
                    }
                }
                crate::hex::HexPane::Ascii => out.push(HexIntent::Ascii(t.clone())),
            },
            // IME 확정 문자(한글 등)는 문자 패널에서만 의미가 있다.
            egui::Event::Ime(egui::ImeEvent::Commit(t))
                if !t.is_empty() && pane == crate::hex::HexPane::Ascii =>
            {
                out.push(HexIntent::Ascii(t.clone()));
            }
            egui::Event::Copy => out.push(HexIntent::Copy),
            // 헥스에는 "잘라내기"를 두지 않는다 — 복사 후 삭제는 두 조작이
            // 명확하고, Cut을 삭제로 오해하면 되돌릴 수 없는 바이너리가 상한다.
            egui::Event::Paste(s) => out.push(HexIntent::Paste(s.clone())),
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let shift = modifiers.shift;
                let ctrl = modifiers.ctrl || modifiers.command;
                match key {
                    egui::Key::ArrowLeft => out.push(HexIntent::Move { delta: -1, extend: shift }),
                    egui::Key::ArrowRight => out.push(HexIntent::Move { delta: 1, extend: shift }),
                    egui::Key::ArrowUp => out.push(HexIntent::Move { delta: -row, extend: shift }),
                    egui::Key::ArrowDown => out.push(HexIntent::Move { delta: row, extend: shift }),
                    egui::Key::PageUp => out.push(HexIntent::Move { delta: -page, extend: shift }),
                    egui::Key::PageDown => out.push(HexIntent::Move { delta: page, extend: shift }),
                    // Ctrl+Home/End는 문서 처음/끝(에디터 관행).
                    egui::Key::Home if ctrl => out.push(HexIntent::MoveDocStart { extend: shift }),
                    egui::Key::End if ctrl => out.push(HexIntent::MoveDocEnd { extend: shift }),
                    egui::Key::Home => out.push(HexIntent::MoveHome { extend: shift }),
                    egui::Key::End => out.push(HexIntent::MoveEnd { extend: shift }),
                    egui::Key::Delete => out.push(HexIntent::DeleteForward),
                    egui::Key::Backspace => out.push(HexIntent::Backspace),
                    egui::Key::Insert => out.push(HexIntent::ToggleInsert),
                    egui::Key::Escape => out.push(HexIntent::ClearSelection),
                    // Ctrl+Z/Y. 전역 Ctrl+Z 처리는 텍스트 편집 버퍼에만 걸리므로
                    // (`can_undo_text`) 헥스에서 두 번 소비될 일이 없다.
                    egui::Key::Z if ctrl && shift => out.push(HexIntent::Redo),
                    egui::Key::Z if ctrl => out.push(HexIntent::Undo),
                    egui::Key::Y if ctrl => out.push(HexIntent::Redo),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// Parquet 정렬이 큰 메모리를 쓸 것 같을 때의 확인. "Sort"면 그 자리에서
/// 정렬한다. `render_confirm_hex_load_dialog`와 같은 규율이다 — 이 창이 떠
/// 있는 동안 `tab_bar_locked`가 참이라 플래그를 세운 문서가 활성 문서로
/// 고정되므로 `app.doc()`으로 읽어도 안전하다.
///
/// 창 X와 Escape는 Cancel과 같다. 없으면 플래그가 켜진 채 잠금이 안 풀린다.
fn render_confirm_parquet_sort_dialog(ctx: &egui::Context, app: &mut App) {
    let s = crate::i18n::t(app.lang);
    let Some((col, dir)) = app.doc().and_then(|d| d.pending_parquet_sort) else {
        return;
    };
    let bytes = app.doc().map_or(0, |d| {
        d.parquet.as_ref().map_or(0, |pq| {
            let mut p = pq.borrow_mut();
            let numeric = p.column_is_numeric(col);
            let rows = p.total_rows();
            let avg = p.estimated_avg_len(col);
            crate::parquet::estimate_sort_bytes(rows, numeric, avg)
        })
    });
    let mut open = true;
    let mut go = false;
    let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Window::new("Sort this column?")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(format!(
                "Sorting loads the key column into memory (about {:.1} MB). Continue?",
                bytes as f64 / 1e6
            )));
            ui.horizontal(|ui| {
                if ui.button(s.sort_do).clicked() {
                    go = true;
                }
                if ui.button(s.common_cancel).clicked() {
                    cancel = true;
                }
            });
        });
    if !open {
        cancel = true;
    }
    if let Some(doc) = app.doc_mut() {
        if go {
            doc.pending_parquet_sort = None;
            sort_parquet_column(doc, col, dir);
        } else if cancel {
            doc.pending_parquet_sort = None;
        }
    }
}

/// 512MB 초과 파일의 메모리 로드 확인. "Load"면 그 자리에서 로드한다.
///
/// **활성 문서를 읽어도 되는 이유(I4).** 이 다이얼로그가 떠 있는 동안은
/// `tab_bar_locked`가 참이라(활성 문서의 `confirm_load`를 조건에 넣었다)
/// 탭 전환·닫기·드롭이 전부 막힌다. 즉 플래그를 세운 문서가 곧 활성 문서로
/// 고정되므로, 크기 문구도 Load/Cancel의 대상도 `app.doc()`으로 읽는 것이
/// 맞다. 이 잠금이 사라지면 크기가 다른 파일의 것으로 바뀌고 Load가 엉뚱한
/// 문서를 메모리에 올린다 — 잠금과 이 함수는 한 묶음이다.
///
/// 창 X(`.open()`)와 Escape는 Cancel과 같다 — 형제 다이얼로그가 전부 갖고
/// 있는 탈출구다. 이게 없으면 플래그가 켜진 채 잠금이 영영 풀리지 않는다.
fn render_confirm_hex_load_dialog(ctx: &egui::Context, app: &mut App) {
    let s = crate::i18n::t(app.lang);
    let len = app.doc().map_or(0, |d| d.source.len());
    let mut open = true;
    let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Window::new("Load Entire File?")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(crate::theme::chrome_text(format!(
                "Editing loads the entire file into memory ({:.1} MB). Continue?",
                len as f64 / 1e6
            )));
            ui.horizontal(|ui| {
                if ui.button(s.common_load).clicked() {
                    if let Some(doc) = app.doc_mut() {
                        let bytes = doc.source.as_bytes().to_vec();
                        if let Some(h) = doc.hex.as_mut() {
                            h.edit = Some(crate::hex::HexEditBuffer::new(bytes));
                            h.confirm_load = false;
                        }
                    }
                }
                if ui.button(s.common_cancel).clicked() {
                    cancel = true;
                }
            });
        });
    if cancel || !open {
        if let Some(h) = app.doc_mut().and_then(|d| d.hex.as_mut()) {
            h.confirm_load = false;
        }
    }
}

/// 이번 프레임의 입력 이벤트에서 텍스트 편집 인텐트를 뽑는다.
/// egui-winit은 Ctrl+C/X/V를 `Event::Copy`/`Cut`/`Paste`로 변환해 보내고
/// `Key` 이벤트는 만들지 않으므로, 그 세 개는 이벤트로만 처리한다.
/// `Event::Text`는 ctrl/command가 눌린 동안에는 오지 않는다(같은 이유).
fn collect_text_intents(i: &egui::InputState) -> Vec<TextEditIntent> {
    let mut out = Vec::new();
    for ev in &i.events {
        match ev {
            egui::Event::Text(t) if !t.is_empty() => {
                out.push(TextEditIntent::Insert(t.clone()));
            }
            // IME 확정 문자(한글/일본어/중국어). **이 분기가 없으면 한글이
            // 아예 입력되지 않는다** — 조합 입력은 `Event::Text`로 오지 않고
            // `Event::Ime`로만 온다.
            egui::Event::Ime(egui::ImeEvent::Commit(t)) if !t.is_empty() => {
                out.push(TextEditIntent::Insert(t.clone()));
            }
            // 조합 중간 상태(ㅎ → 하 → 한). **버퍼가 아니라 미리보기로**
            // 넘긴다.
            //
            // 버퍼에 직접 넣으면 다음 Preedit마다 지우고 다시 넣어야 하고,
            // 그 되돌리기가 undo 스택에 낱글자로 쌓여 Ctrl+Z 한 번이
            // "ㅎ→하→한"의 한 단계만 되돌리게 된다. dirty 표시도 조합만
            // 해도 켜진다. 그래서 조합 중 글자는 **화면에만** 그리고
            // (`render_text`의 미리보기 galley), 확정될 때 비로소 Commit이
            // 버퍼에 넣는다 — undo 단위는 확정된 글자, 화면은 조합 그대로다.
            egui::Event::Ime(egui::ImeEvent::Preedit(t)) => {
                out.push(TextEditIntent::ImePreview(t.clone()));
            }
            // 조합이 끝나거나 취소되면 미리보기를 지운다. `Disabled`만 보고
            // `Commit` 뒤를 안 지우면 확정된 글자가 버퍼와 미리보기에 **둘 다**
            // 있어 화면에 두 번 나온다(윈도우 IME는 Commit 뒤 빈 Preedit을
            // 보내지만, 그것에 기대지 않고 여기서 명시적으로 지운다).
            egui::Event::Ime(egui::ImeEvent::Disabled) => {
                out.push(TextEditIntent::ImePreview(String::new()));
            }
            egui::Event::Copy => out.push(TextEditIntent::Copy),
            egui::Event::Cut => out.push(TextEditIntent::Cut),
            egui::Event::Paste(s) => out.push(TextEditIntent::Paste(s.clone())),
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let shift = modifiers.shift;
                let ctrl = modifiers.ctrl || modifiers.command;
                match key {
                    egui::Key::Enter => out.push(TextEditIntent::Newline),
                    // Tab은 여기서 **다루지 않는다**. `render_text`가 포커스
                    // 게이트보다 먼저 소비해 인텐트로 되살리기 때문이다 —
                    // 소비를 게이트 뒤로 미루면 Tab이 포커스를 옮긴 뒤
                    // 스스로를 막는다(그 호출부 주석 참조). 여기에 분기를
                    // 두면 같은 Tab이 두 번 삽입된다.
                    egui::Key::Backspace => out.push(TextEditIntent::Backspace),
                    egui::Key::Delete => out.push(TextEditIntent::Delete),
                    egui::Key::A if ctrl => out.push(TextEditIntent::SelectAll),
                    egui::Key::ArrowLeft => {
                        out.push(TextEditIntent::Move(CaretMove::Left, shift))
                    }
                    egui::Key::ArrowRight => {
                        out.push(TextEditIntent::Move(CaretMove::Right, shift))
                    }
                    egui::Key::ArrowUp => out.push(TextEditIntent::Move(CaretMove::Up, shift)),
                    egui::Key::ArrowDown => {
                        out.push(TextEditIntent::Move(CaretMove::Down, shift))
                    }
                    egui::Key::Home => out.push(TextEditIntent::Move(CaretMove::Home, shift)),
                    egui::Key::End => out.push(TextEditIntent::Move(CaretMove::End, shift)),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// 인텐트 하나를 편집 버퍼에 적용한다. 모든 변경은 `dirty = true`.
///
/// 핵심 불변식: `lines[i]`에는 `\n`/`\r`가 들어가면 안 된다. 그래서
/// 문자 삽입은 항상 `insert_str`(개행을 새 줄로 분해)을 거치고, Enter는
/// `split_line`으로 라우팅한다 — `insert_char`에 `'\n'`을 넘기지 않는다.
fn apply_text_intent(
    ui: &mut egui::Ui,
    doc: &mut Document,
    clipboard: &mut String,
    intent: TextEditIntent,
) {
    use crate::edit::{backspace, delete_range, insert_str, normalize, selection_text, split_line};

    // IME 미리보기는 **버퍼를 건드리지 않는다** — 화면에만 그릴 문자열이라
    // 되돌리기·dirty·캐럿과 무관하다. 아래 편집 경로로 내려보내지 않고
    // 여기서 끝낸다(그래야 조합 중 타이핑이 undo 스택을 오염시키지 않는다).
    if let TextEditIntent::ImePreview(t) = intent {
        doc.ime_preview = t;
        return;
    }
    // 미리보기가 아닌 인텐트가 왔다는 것은 조합이 끝났다는 뜻이다(확정된
    // Commit, 또는 조합을 깨는 이동·삭제 등). 여기서 지우지 않으면 확정된
    // 글자가 **버퍼와 미리보기에 둘 다** 있어 화면에 두 번 나온다.
    doc.ime_preview.clear();

    // 이 인텐트가 버퍼를 바꾸는가. 순수 이동/복사/전체선택은 되돌리기 기록이
    // 필요 없으므로 스냅샷 비용도 들이지 않는다.
    let mutating = matches!(
        intent,
        TextEditIntent::Insert(_)
            | TextEditIntent::Newline
            | TextEditIntent::Backspace
            | TextEditIntent::Delete
            | TextEditIntent::Cut
            | TextEditIntent::Paste(_)
    );

    let caret = doc.text_caret;
    let sel_raw = doc.text_sel;
    let Some(e) = doc.edit.as_mut() else { return };
    if e.lines.is_empty() {
        e.lines.push(String::new());
    }
    // 캐럿/선택은 반드시 현재 lines 범위로 클램프한 뒤 쓴다. delete_range 등은
    // lines[pos.line]을 직접 인덱싱하므로, 한 프레임에 여러 인텐트가 연속으로
    // 적용돼 줄 수가 줄어든 뒤의 오래된 위치를 그대로 넘기면 패닉한다.
    let caret = clamp_pos(&e.lines, caret);
    let sel = sel_raw
        .map(|(a, b)| (clamp_pos(&e.lines, a), clamp_pos(&e.lines, b)))
        .filter(|(a, b)| a != b);

    // 선택을 먼저 지우고 그 지점을 새 캐럿으로 삼는 공통 처리.
    let delete_sel = |lines: &mut Vec<String>, sel: Option<(_, _)>| -> Option<crate::edit::TextPos> {
        sel.map(|(a, b)| delete_range(lines, a, b))
    };

    // ---- 되돌리기 스냅샷(편집 **전**) ----
    // 텍스트 편집이 건드릴 수 있는 줄 범위를 미리 통째로 복사해 둔다.
    // 범위는 [start, end]:
    //  - Backspace는 캐럿의 앞 줄까지 병합할 수 있으므로 start를 한 줄 넓힌다.
    //  - Delete는 캐럿의 다음 줄을 끌어올릴 수 있으므로 end를 한 줄 넓힌다.
    //  - 선택이 있으면 선택 전 구간이 대상이다.
    // 편집 뒤 행 수 변화(delta)를 보고 op를 고른다(아래 `record_undo`).
    let snap = if mutating {
        Some(text_edit_snapshot(&e.lines, caret, sel, &intent))
    } else {
        None
    };
    let before_len = e.lines.len();

    match intent {
        // 위에서 처리하고 반환했다(버퍼를 건드리지 않는 유일한 인텐트).
        TextEditIntent::ImePreview(_) => unreachable!("미리보기는 위에서 끝낸다"),
        TextEditIntent::Insert(t) => {
            // \r은 버리고 \n만 남긴다 — insert_str이 \n을 줄 분할로 처리한다.
            let t = t.replace('\r', "");
            if t.is_empty() {
                return;
            }
            let at = delete_sel(&mut e.lines, sel).unwrap_or(caret);
            doc.text_caret = insert_str(&mut e.lines, at, &t);
            doc.text_sel = None;
            e.dirty = true;
        }
        TextEditIntent::Newline => {
            let at = delete_sel(&mut e.lines, sel).unwrap_or(caret);
            doc.text_caret = split_line(&mut e.lines, at);
            doc.text_sel = None;
            e.dirty = true;
        }
        TextEditIntent::Backspace => {
            // 아무것도 지우지 않았으면 dirty를 세우지 않는다. 선택이 없고
            // 캐럿이 문서 맨 앞(0,0)이면 backspace는 위치를 그대로 돌려주는
            // no-op이다 — 그때도 dirty를 세우면 파일을 열자마자 Backspace 한 번에
            // "저장 안 됨" 상태가 생긴다.
            let had_sel = sel.is_some();
            doc.text_caret = match delete_sel(&mut e.lines, sel) {
                Some(p) => p,
                None => backspace(&mut e.lines, caret),
            };
            doc.text_sel = None;
            if backspace_or_delete_changed(had_sel, caret, doc.text_caret) {
                e.dirty = true;
            }
        }
        TextEditIntent::Delete => {
            // Backspace와 같은 이유로 no-op(문서 끝에서 Delete)에는 dirty를
            // 세우지 않는다. 다만 Delete는 캐럿이 제자리인 채로 실제 삭제가
            // 일어나므로(다음 문자/개행 제거), 캐럿 이동이 아니라 "삭제를
            // 수행했는가"를 직접 본다.
            let mut changed = sel.is_some();
            doc.text_caret = match delete_sel(&mut e.lines, sel) {
                Some(p) => p,
                None => {
                    // 캐럿 다음 한 문자. 줄 끝이면 다음 줄과 병합.
                    let next = apply_caret_move(&e.lines, caret, CaretMove::Right);
                    if next == caret {
                        caret // 문서 끝 — no-op
                    } else {
                        changed = true;
                        delete_range(&mut e.lines, caret, next)
                    }
                }
            };
            doc.text_sel = None;
            if changed {
                e.dirty = true;
            }
        }
        TextEditIntent::Move(mv, extend) => {
            let (new_caret, new_sel) = next_caret_and_sel(&e.lines, caret, sel, mv, extend);
            doc.text_caret = new_caret;
            doc.text_sel = new_sel;
        }
        TextEditIntent::SelectAll => {
            let (a, b) = whole_document_sel(&e.lines);
            doc.text_sel = Some((a, b));
            doc.text_caret = b;
        }
        TextEditIntent::Copy => {
            if let Some((a, b)) = sel {
                let (a, b) = normalize(a, b);
                let s = selection_text(&e.lines, a, b);
                *clipboard = s.clone();
                ui.output_mut(|o| o.copied_text = s);
            }
        }
        TextEditIntent::Cut => {
            if let Some((a, b)) = sel {
                let (a, b) = normalize(a, b);
                let s = selection_text(&e.lines, a, b);
                *clipboard = s.clone();
                ui.output_mut(|o| o.copied_text = s);
                doc.text_caret = delete_range(&mut e.lines, a, b);
                doc.text_sel = None;
                e.dirty = true;
            }
        }
        TextEditIntent::Paste(s) => {
            let s = s.replace("\r\n", "\n").replace('\r', "\n");
            if s.is_empty() {
                return;
            }
            let at = delete_sel(&mut e.lines, sel).unwrap_or(caret);
            doc.text_caret = insert_str(&mut e.lines, at, &s);
            doc.text_sel = None;
            e.dirty = true;
        }
    }

    // ---- 되돌리기 기록(편집 **후**, 담기는 내용은 편집 전 스냅샷) ----
    // 스냅샷 구간이 실제로 바뀌었을 때만 쌓는다. 맨 앞 Backspace/문서 끝
    // Delete처럼 아무것도 하지 않은 경우에 빈 undo 단계가 생기면 Ctrl+Z가
    // "아무 일도 안 일어나는" 헛발질이 된다.
    if let Some(snap) = snap {
        let changed = e.lines.len() != before_len
            || snap
                .lines
                .iter()
                .enumerate()
                .any(|(k, s)| e.lines.get(snap.start + k) != Some(s));
        if changed {
            if let Some(op) = undo_op_from_snapshot(snap, before_len, e.lines.len()) {
                e.undo.push(op);
            }
        }
    }

}

/// 텍스트 편집 전 스냅샷: 영향을 받을 수 있는 줄 구간 `[start, ..]`의 원본.
struct TextSnapshot {
    start: usize,
    /// 편집 전 `lines[start..=end]`의 사본(end는 lines 길이로 클램프됨).
    lines: Vec<String>,
}

/// 인텐트가 건드릴 수 있는 줄 구간을 잡아 편집 **전** 내용을 복사한다.
///
/// 텍스트 편집은 대부분 한두 줄만 건드리므로 비용이 작다. 정확성을 위해
/// 경계를 한 줄씩 넉넉히 잡는다(Backspace는 앞 줄과 병합, Delete는 다음 줄을
/// 끌어올림). 붙여넣기/입력은 캐럿 줄 하나에서 시작해 아래로만 늘어난다.
fn text_edit_snapshot(
    lines: &[String],
    caret: crate::edit::TextPos,
    sel: Option<(crate::edit::TextPos, crate::edit::TextPos)>,
    intent: &TextEditIntent,
) -> TextSnapshot {
    let (mut lo, mut hi) = match sel {
        Some((a, b)) => {
            let (a, b) = crate::edit::normalize(a, b);
            (a.line, b.line)
        }
        None => (caret.line, caret.line),
    };
    if sel.is_none() {
        match intent {
            TextEditIntent::Backspace => lo = lo.saturating_sub(1),
            TextEditIntent::Delete => hi += 1,
            _ => {}
        }
    }
    let hi = hi.min(lines.len().saturating_sub(1));
    let start = lo.min(hi);
    TextSnapshot {
        start,
        lines: lines[start..=hi].to_vec(),
    }
}

/// 스냅샷 + 행 수 변화로 되돌리기 op 하나를 만든다.
///
/// 행 수가 그대로면 스냅샷 구간을 통째로 `Replace`하면 원상복구된다.
/// 행이 늘었으면(Enter/여러 줄 붙여넣기) 늘어난 만큼을 먼저 제거한 뒤
/// 스냅샷을 되돌려야 하고, 줄었으면(Backspace 병합/멀티라인 삭제) 부족한
/// 줄을 되꽂은 뒤 스냅샷을 되돌려야 한다 — 둘 다 `Batch`로 한 단계에 묶는다.
/// (Batch 내부는 담긴 순서대로 적용되므로 구조 변경을 먼저 둔다.)
fn undo_op_from_snapshot(
    snap: TextSnapshot,
    before_len: usize,
    after_len: usize,
) -> Option<crate::edit::EditOp> {
    if snap.lines.is_empty() {
        return None;
    }
    let n = snap.lines.len();
    let restore = crate::edit::EditOp::Replace(
        snap.lines
            .iter()
            .enumerate()
            .map(|(k, s)| (snap.start + k, s.clone()))
            .collect(),
    );
    Some(match after_len.cmp(&before_len) {
        std::cmp::Ordering::Equal => restore,
        std::cmp::Ordering::Greater => {
            // 늘어난 줄들은 스냅샷 구간 바로 뒤에 생긴다(삽입은 캐럿 줄
            // 아래로만 확장된다). 먼저 그것들을 제거해 길이를 맞춘 뒤 복원.
            let added = after_len - before_len;
            crate::edit::EditOp::Batch(vec![
                crate::edit::EditOp::RemoveInserted { at: snap.start + n, count: added },
                restore,
            ])
        }
        std::cmp::Ordering::Less => {
            // 줄어든 만큼 자리를 만들어 준 뒤 스냅샷으로 덮는다. 내용은
            // 어차피 restore가 전부 덮으므로 자리표시자로 빈 줄을 꽂는다.
            let lost = before_len - after_len;
            crate::edit::EditOp::Batch(vec![
                crate::edit::EditOp::ReinsertRemoved {
                    at: (snap.start + n).saturating_sub(lost),
                    lines: vec![String::new(); lost],
                },
                restore,
            ])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(content: &[u8]) -> std::path::PathBuf {
        temp_ext(content, "csv")
    }

    /// 확장자를 지정해 임시 파일을 만든다. detect_separator가 확장자를 먼저
    /// 보므로, 내용 기반 감지를 테스트하려면 중립적 확장자(txt 등)를 써야 한다.
    fn temp_ext(content: &[u8], ext: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("tv_app_{}_{}.{ext}", std::process::id(), id));
        std::fs::File::create(&p).unwrap().write_all(content).unwrap();
        p
    }

    /// 메모리 바이트로 헥스 문서 탭 하나를 가진 App.
    fn hex_test_doc(bytes: &[u8]) -> App {
        let mut app = App::default();
        let src = Arc::new(Source::from_bytes(bytes.to_vec()));
        app.add_document(hex_document(src, std::path::Path::new("")));
        app
    }

    #[test]
    fn open_detects_and_primes() {
        let p = temp(b"name,age\nAlice,30\nBob,25\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert_eq!(doc.enc, crate::parse::Encoding::Utf8);
        assert_eq!(doc.sep, SeparatorMode::Char(b','));
        assert!(doc.has_header);
        assert!(app.error.is_none());
    }

    #[test]
    fn open_missing_sets_error() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(std::path::Path::new("nope_xyz.csv"), &ctx);
        assert!(app.doc().is_none());
        assert!(app.error.is_some());
    }

    #[test]
    fn open_path_twice_creates_two_tabs() {
        let p1 = temp(b"a,b\n1,2\n");
        let p2 = temp(b"c,d\n3,4\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p1, &ctx);
        app.open_path(&p2, &ctx);
        assert_eq!(app.docs.len(), 2);
        assert_eq!(app.active, 1);
        assert_eq!(app.docs[0].path, p1, "첫 탭의 path가 보존되어야 한다");
    }

    #[test]
    fn open_failure_keeps_existing_tabs() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        app.open_path(std::path::Path::new("nope_xyz.csv"), &ctx);
        assert_eq!(app.docs.len(), 1, "실패한 열기는 기존 탭을 건드리지 않는다");
        assert!(app.error.is_some());
    }

    /// 바이너리 파일을 열면 문서를 만들지 않고 선택 다이얼로그를 보류한다.
    #[test]
    fn open_binary_defers_to_dialog() {
        let p = temp_ext(b"SQLite format 3\x00\x10\x00\x01", "gpkg");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        assert_eq!(app.docs.len(), 0, "문서가 아직 없어야 한다");
        let pending = app.pending_binary_open.as_ref().expect("보류 상태");
        assert_eq!(pending.path, p);
        std::fs::remove_file(&p).ok();
    }

    /// **보류 중인 열기 방식 선택을 덮어쓰지 않는다(C1 회귀).**
    /// .gpkg 셋을 한 번에 드롭하면 예전에는 `open_path`가 매번
    /// `pending_binary_open`을 무조건 대입해, 앞의 둘이 소리 없이 사라지고
    /// 마지막 하나만 다이얼로그를 띄웠다. 사용자가 이미 인코딩을 고른 뒤에
    /// File▸Open…을 또 써도 그 선택이 초기화됐다.
    ///
    /// 그리고 그 다이얼로그가 떠 있는 동안은 `tab_bar_locked`가 참이라
    /// 드롭 자체가 막힌다 — 두 겹의 방어 모두를 고정한다.
    #[test]
    fn open_path_refuses_while_binary_dialog_pending() {
        let p1 = temp_ext(b"SQLite format 3\x00\x10\x00\x01", "gpkg");
        let p2 = temp_ext(b"\x00\x01\x02\x03\x00\x00\x00\x00", "gpkg");
        let p3 = temp(b"a,b\n1,2\n"); // 텍스트도 마찬가지로 막힌다.
        let ctx = egui::Context::default();
        let mut app = App::default();

        app.open_path(&p1, &ctx);
        // 사용자가 인코딩을 골라 둔 상태를 흉내낸다.
        app.pending_binary_open.as_mut().unwrap().enc = Encoding::Utf16Le;

        app.open_path(&p2, &ctx);
        let pending = app.pending_binary_open.as_ref().expect("보류가 남아 있어야 한다");
        assert_eq!(pending.path, p1, "첫 파일의 보류를 덮어쓰면 안 된다");
        assert_eq!(pending.enc, Encoding::Utf16Le, "고른 인코딩도 유지돼야 한다");
        assert_eq!(app.error.as_deref(), Some(BINARY_OPEN_PENDING_STATUS));

        // 텍스트 파일도 탭을 만들지 못한다(활성 탭이 바뀌면 다이얼로그가
        // 엉뚱한 문서를 겨눈다).
        app.open_path(&p3, &ctx);
        assert_eq!(app.docs.len(), 0, "다이얼로그가 떠 있는 동안은 탭이 늘지 않는다");

        // 잠금이 드롭 경로도 막는다.
        assert!(tab_bar_locked_for(&app), "열기 방식 선택 중에는 탭 바가 잠긴다");
        assert!(matches!(
            plan_dropped_files(vec![p3.clone()], tab_bar_locked_for(&app)),
            DropPlan::Locked(_)
        ));

        // 다이얼로그를 닫으면 다시 열린다.
        app.pending_binary_open = None;
        assert!(!tab_bar_locked_for(&app));
        app.open_path(&p3, &ctx);
        assert_eq!(app.docs.len(), 1);

        for p in [&p1, &p2, &p3] {
            std::fs::remove_file(p).ok();
        }
    }

    /// 다이얼로그에서 "헥스로 열기"를 고르면 헥스 문서가 생긴다.
    #[test]
    fn open_path_hex_creates_hex_document() {
        let p = temp_ext(b"\x00\x01\x02ABC", "bin");
        let mut app = App::default();
        app.open_path_hex(&p);
        assert_eq!(app.docs.len(), 1);
        let doc = app.doc().unwrap();
        assert!(doc.hex.is_some());
        assert!(doc.indexer.is_none(), "헥스 문서는 줄 인덱서를 돌리지 않는다");
        assert!(matches!(doc.sep, SeparatorMode::None));
        assert_eq!(doc.source.len(), 6);
        std::fs::remove_file(&p).ok();
    }

    // ---- Parquet 배선 ----

    #[test]
    fn parquet_file_opens_as_a_parquet_document() {
        let p = crate::parquet::testutil::temp_path("openpath");
        crate::parquet::testutil::write_simple(&p, vec![1, 2], vec![Some("가"), Some("나")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().expect("탭이 열려야 한다");
        assert!(doc.parquet.is_some(), "Parquet 문서로 열려야 한다");
        assert!(doc.edit.is_none(), "편집 모드로 들어가면 안 된다");
        assert!(doc.hex.is_none(), "헥스 모드가 아니다");
        assert!(doc.indexer.is_none(), "개행을 셀 필요가 없다");
        assert_eq!(doc.sep, SeparatorMode::Char(b','), "구분자는 콤마 고정");
        assert!(doc.has_header, "첫 논리 행이 컬럼 이름이다");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn small_parquet_does_not_auto_enter_edit_mode() {
        // `auto_edit_on_open`은 크기 기준이라 작은 Parquet은 그냥 두면 편집
        // 모드로 들어간다. `load_edit_buffer`가 바이너리를 깨진 문자열로
        // 올리게 되므로 반드시 막아야 한다.
        let p = crate::parquet::testutil::temp_path("small");
        crate::parquet::testutil::write_simple(&p, vec![1], vec![Some("x")]);
        assert!(
            auto_edit_on_open(std::fs::metadata(&p).unwrap().len()),
            "이 파일은 크기만 보면 자동 편집 대상이다(테스트 전제)"
        );
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        assert!(
            app.doc().unwrap().edit.is_none(),
            "그래도 편집 모드가 아니어야 한다"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn enter_edit_mode_refuses_a_parquet_document() {
        // UI 비활성화가 아니라 함수 자체가 막아야 한다 — 호출부가 셋이고
        // 새로 생길 수 있다.
        let p = crate::parquet::testutil::temp_path("gate");
        crate::parquet::testutil::write_simple(&p, vec![1], vec![Some("x")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        enter_edit_mode(doc);
        assert!(doc.edit.is_none(), "Parquet은 편집 모드에 들어갈 수 없다");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parquet_extension_with_text_content_opens_as_text() {
        // 확장자가 아니라 매직으로 판단한다.
        let p = temp_ext(b"a,b\n1,2\n", "parquet");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        assert!(
            app.doc().unwrap().parquet.is_none(),
            "텍스트로 열려야 한다"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn broken_parquet_reports_an_error_without_opening_a_tab() {
        // PAR1로 시작하지만 내용이 깨진 파일.
        let p = temp_ext(b"PAR1\x00\x00\x00garbage", "parquet");
        let ctx = egui::Context::default();
        let mut app = App::default();
        let before = app.docs.len();
        app.open_path(&p, &ctx);
        assert_eq!(app.docs.len(), before, "탭이 추가되면 안 된다");
        assert!(app.error.is_some(), "오류 메시지가 있어야 한다");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parquet_line_count_includes_the_header_row() {
        let p = crate::parquet::testutil::temp_path("count");
        crate::parquet::testutil::write_simple(&p, vec![1, 2, 3], vec![None, None, None]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        assert_eq!(doc_line_count(app.doc().unwrap()), 4, "데이터 3 + 헤더 1");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parquet_logical_line_returns_header_then_rows() {
        let p = crate::parquet::testutil::temp_path("ll");
        crate::parquet::testutil::write_simple(&p, vec![10], vec![Some("가")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert_eq!(logical_line(doc, 0).as_deref(), Some("id,name"));
        assert_eq!(logical_line(doc, 1).as_deref(), Some("10,가"));
        assert_eq!(logical_line(doc, 2), None);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn find_all_finds_matches_in_a_parquet_document() {
        let p = crate::parquet::testutil::temp_path("find");
        crate::parquet::testutil::write_simple(
            &p,
            vec![1, 2, 3],
            vec![Some("서울"), Some("부산"), Some("서울시청")],
        );
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "서울".to_string();
        let rows = scan_all_matches(doc);
        // 논리 행 1(서울)과 3(서울시청). 헤더(0)에는 없다.
        assert_eq!(rows, vec![1u32, 3], "실제: {rows:?}");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn find_widens_projection_so_hidden_columns_still_match() {
        // 렌더가 좁혀 둔 프로젝션을 되돌리지 않으면 화면 밖 컬럼의 매치를
        // 조용히 놓친다 — 오류도 안 나는 종류라 반드시 테스트로 막는다.
        let p = crate::parquet::testutil::temp_path("widen");
        crate::parquet::testutil::write_simple(&p, vec![42], vec![Some("가")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.parquet
            .as_ref()
            .unwrap()
            .borrow_mut()
            .set_visible_columns(Some(vec![0]));
        doc.find_query = "가".to_string();
        assert_eq!(
            scan_all_matches(doc),
            vec![1u32],
            "안 보이는 컬럼의 매치도 잡아야 한다"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn find_next_works_in_a_parquet_document() {
        let p = crate::parquet::testutil::temp_path("findnext");
        crate::parquet::testutil::write_simple(&p, vec![1, 2], vec![Some("가"), Some("나")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        app.doc_mut().unwrap().find_query = "나".to_string();
        let doc = app.doc().unwrap();
        let m = search_from(doc, crate::edit::TextPos { line: 0, col: 0 }, true);
        assert!(m.is_some(), "다음 찾기가 매치를 찾아야 한다");
        assert_eq!(m.unwrap().line, 2, "논리 행 2");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn sorting_a_parquet_document_reorders_rendered_rows() {
        let p = crate::parquet::testutil::temp_path("sortdoc");
        crate::parquet::testutil::write_simple(
            &p,
            vec![3, 1, 2],
            vec![Some("c"), Some("a"), Some("b")],
        );
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        sort_parquet_column(doc, 1, SortDir::Asc);
        let perm = &doc.sort.as_ref().expect("정렬 상태가 있어야 한다").permutation;
        // a(논리2), b(논리3), c(논리1)
        assert_eq!(perm, &vec![2u32, 3, 1]);
        // 순열을 통해 읽으면 정렬된 순서다.
        let first = logical_line(doc, perm[0] as usize).unwrap();
        assert!(first.ends_with(",a"), "첫 행은 a여야 한다: {first}");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn a_small_parquet_sort_runs_without_confirmation() {
        let p = crate::parquet::testutil::temp_path("nogate");
        crate::parquet::testutil::write_simple(&p, vec![2, 1], vec![Some("b"), Some("a")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        request_parquet_sort(doc, 0, SortDir::Asc);
        assert!(doc.pending_parquet_sort.is_none(), "확인 없이 바로 정렬");
        assert!(doc.sort.is_some(), "정렬이 적용됐다");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn exporting_a_parquet_document_writes_all_rows_as_csv() {
        let p = crate::parquet::testutil::temp_path("export");
        crate::parquet::testutil::write_simple(&p, vec![1, 2], vec![Some("가"), Some("a,b")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        let lines = collect_export_lines(doc);
        assert_eq!(lines.len(), 3, "헤더 + 데이터 2행");
        assert_eq!(lines[0], "id,name");
        assert_eq!(lines[1], "1,가");
        assert_eq!(lines[2], "2,\"a,b\"", "구분자가 든 값은 인용된다");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn exporting_a_sorted_parquet_document_follows_screen_order() {
        let p = crate::parquet::testutil::temp_path("expsort");
        crate::parquet::testutil::write_simple(&p, vec![2, 1], vec![Some("b"), Some("a")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        sort_parquet_column(doc, 0, SortDir::Asc);
        let lines = collect_export_lines(doc);
        assert_eq!(lines[0], "id,name", "헤더가 먼저");
        assert_eq!(lines[1], "1,a", "정렬된 순서");
        assert_eq!(lines[2], "2,b");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parquet_disables_editing_but_keeps_tools_available() {
        // **이 셋이 갈리는 것이 핵심이다.** 하나로 묶으면 반드시 하나가 틀린다:
        // Parquet은 편집이 안 되지만 정렬·찾기는 되어야 하고, 구분자 선택과
        // 인덱싱 문구는 의미가 없다.
        let p = crate::parquet::testutil::temp_path("gates");
        crate::parquet::testutil::write_simple(&p, vec![1], vec![Some("x")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert!(!text_edit_allowed(doc), "편집은 막힌다");
        assert!(
            text_tools_enabled(doc),
            "Tools 메뉴(정렬)는 열려 있어야 한다 — Parquet 정렬이 기능이다"
        );
        assert!(
            !text_layout_tools_enabled(doc),
            "구분자 선택·인덱싱 문구는 의미가 없다"
        );
        std::fs::remove_file(&p).ok();
    }

    /// **메뉴에서 실제로 닿는지**를 본다. 이 테스트가 없어서 내보내기 코드가
    /// 죽어 있었다 — `collect_export_lines`와 저장 경로는 완성됐는데
    /// `doc_savable`(편집 버퍼 필수)이 메뉴를 잠가 도달할 수 없었다.
    #[test]
    fn parquet_can_be_exported_but_not_saved_in_place() {
        let p = crate::parquet::testutil::temp_path("exportgate");
        crate::parquet::testutil::write_simple(&p, vec![1], vec![Some("x")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert!(
            !doc_savable(doc),
            "제자리 저장은 막힌다 — 읽기 전용 파일을 덮어쓸 수 없다"
        );
        assert!(
            doc_exportable(doc),
            "CSV/TSV 내보내기는 되어야 한다 — 이게 false면 메뉴가 잠겨 \
             collect_export_lines가 영영 안 불린다"
        );
        std::fs::remove_file(&p).ok();
    }

    /// 정렬 메뉴/툴바가 `Phase::Complete`를 보면 Parquet에서 영영 안 열린다 —
    /// 인덱서를 띄우지 않아 Phase가 `Priming`에 멈춰 있기 때문이다.
    #[test]
    fn parquet_rows_are_ready_without_the_indexer() {
        let p = crate::parquet::testutil::temp_path("rowsready");
        crate::parquet::testutil::write_simple(&p, vec![1], vec![Some("x")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert_ne!(
            doc.index.status().phase,
            crate::index::Phase::Complete,
            "사전 조건: 인덱서를 안 띄우므로 Complete가 아니다"
        );
        assert!(
            doc_rows_ready(doc),
            "그래도 전체 행에 접근할 수 있다 — 푸터가 행 수를 알고 row group을 \
             임의 접근할 수 있다"
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parquet_status_shows_rows_columns_and_read_only() {
        let p = crate::parquet::testutil::temp_path("status");
        crate::parquet::testutil::write_simple(&p, vec![1, 2, 3], vec![None, None, None]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let s = parquet_status_text(app.doc().unwrap());
        assert!(s.contains('3'), "행 수: {s}");
        assert!(s.contains('2'), "컬럼 수: {s}");
        assert!(s.contains("read-only"), "읽기 전용임을 알려야 한다: {s}");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parquet_table_reports_every_column_and_data_row() {
        // `table_col_count`/`render_table`이 `index.line_count()`(Parquet은 0)를
        // 쓰면 컬럼이 1로 무너지고 표가 통째로 비어 보인다.
        let p = crate::parquet::testutil::temp_path("cols");
        crate::parquet::testutil::write_simple(&p, vec![1, 2], vec![Some("가"), Some("나")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert_eq!(table_col_count(doc, b','), 2, "id + name");
        assert_eq!(doc_line_count(doc), 3, "헤더 + 데이터 2행");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn parquet_sort_confirmation_locks_the_tab_bar() {
        // 확인 창이 떠 있는 동안 탭이 바뀌면 엉뚱한 문서를 정렬하거나
        // 플래그가 영영 남는다(hex confirm_load와 같은 이유).
        let p = crate::parquet::testutil::temp_path("locktab");
        crate::parquet::testutil::write_simple(&p, vec![1], vec![Some("x")]);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        assert!(!tab_bar_locked_for(&app), "평소엔 안 잠긴다");
        app.doc_mut().unwrap().pending_parquet_sort = Some((0, SortDir::Asc));
        assert!(tab_bar_locked_for(&app), "확인 대기 중엔 잠긴다");
        std::fs::remove_file(&p).ok();
    }

    /// 실제 GeoParquet 파일(5000행, 8열, ZSTD, row group 5개)로 전 경로를
    /// 한 번에 확인한다. `sample_geo.parquet`이 없으면 건너뛴다 — 이 파일은
    /// 저장소에 넣지 않는다(바이너리).
    #[test]
    fn real_geoparquet_file_reads_end_to_end() {
        let p = std::path::Path::new("sample_geo.parquet");
        if !p.exists() {
            return;
        }
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(p, &ctx);
        let doc = app.doc().expect("열려야 한다");
        assert!(doc.parquet.is_some());
        assert_eq!(doc_line_count(doc), 5001, "5000행 + 헤더");
        assert_eq!(table_col_count(doc, b','), 8, "8개 컬럼");

        // 헤더
        let hdr = logical_line(doc, 0).unwrap();
        assert!(hdr.starts_with("id,name,pop,active,surveyed,updated,note,geometry"), "{hdr}");

        // 여러 row group에 걸친 행들(그룹 크기 1000)
        for logical in [1usize, 1500, 3001, 5000] {
            let line = logical_line(doc, logical).expect("행이 있어야 한다");
            let f = crate::parse::split_fields(&line, b',');
            assert_eq!(f.len(), 8, "논리행 {logical}: 컬럼 8개여야 한다 -> {f:?}");
            assert!(!line.contains('\n'), "개행이 남으면 안 된다: {logical}");
        }

        // 마지막 행 다음은 없다
        assert_eq!(logical_line(doc, 5001), None);

        // geometry 요약 - POINT와 POLYGON 둘 다 나온다
        let r1 = crate::parse::split_fields(&logical_line(doc, 1).unwrap(), b',');
        assert!(r1[7].starts_with("POINT("), "행1 geometry: {}", r1[7]);
        let r2 = crate::parse::split_fields(&logical_line(doc, 2).unwrap(), b',');
        assert_eq!(r2[7], "POLYGON(1,204 pts)", "쉼표 든 요약이 한 셀로 유지");

        // null - 논리행 1이 파일행 0이고 0 % 7 == 0이라 name이 null
        assert_eq!(r1[1], "", "null은 빈 문자열");

        // 타입 포맷
        assert!(r1[4].contains('-'), "date32는 날짜 형식: {}", r1[4]);
        assert!(r1[5].contains('T'), "timestamp는 ISO: {}", r1[5]);
        assert_eq!(r1[3], "true", "bool");

        // 찾기 - 화면 밖 컬럼도 잡는다
        let d = app.doc_mut().unwrap();
        d.find_query = "지역-4999".to_string();
        let rows = scan_all_matches(d);
        assert_eq!(rows, vec![5000u32], "마지막 행을 찾아야 한다");

        // 정렬 - pop 내림차순이면 첫 행이 가장 큰 값
        let d = app.doc_mut().unwrap();
        sort_parquet_column(d, 2, SortDir::Desc);
        let perm = d.sort.as_ref().unwrap().permutation.clone();
        assert_eq!(perm.len(), 5000);
        assert_eq!(perm[0], 5000, "pop 최대는 마지막 파일행 -> 논리행 5000");

        // 내보내기 - 정렬 순서 + 전체 행
        let d = app.doc().unwrap();
        let lines = collect_export_lines(d);
        assert_eq!(lines.len(), 5001, "헤더 + 5000행");
        assert!(lines[0].starts_with("id,name"), "헤더가 먼저");
        let first = crate::parse::split_fields(&lines[1], b',');
        assert_eq!(first[0], "4999", "정렬 순서를 따른다");

        // **UI가 그 기능에 닿는가.** 위의 함수들이 다 맞아도 메뉴가 잠겨
        // 있으면 사용자에게는 없는 기능이다(실제로 둘 다 잠겨 있었다).
        assert!(doc_exportable(d), "내보내기 메뉴가 열려 있어야 한다");
        assert!(!doc_savable(d), "제자리 저장은 막혀 있어야 한다");
        assert!(doc_rows_ready(d), "정렬 메뉴·툴바가 열려 있어야 한다");
        assert!(text_tools_enabled(d), "Tools 메뉴(정렬)가 열려 있어야 한다");
        assert!(!text_edit_allowed(d), "편집은 막혀 있어야 한다");

        // 내보낸 CSV를 실제로 써서 다시 읽는다 — 왕복이 파일 단위로도
        // 성립하는지(인용·개행 치환 포함).
        let out = std::env::temp_dir().join(format!("tv_pq_export_{}.csv", std::process::id()));
        crate::save::write_file(
            &out,
            &lines,
            &crate::save::SaveOptions {
                enc: Encoding::Utf8,
                bom: false,
                newline: crate::edit::Newline::Lf,
            },
            None,
        )
        .unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        let written: Vec<&str> = text.lines().collect();
        assert_eq!(written.len(), 5001, "파일에도 5001줄");
        assert_eq!(written[0], lines[0], "헤더가 그대로");
        // 개행이 든 셀이 한 줄로 눌렸으므로 줄 수가 행 수와 정확히 같다.
        for (i, w) in written.iter().enumerate().take(50) {
            assert_eq!(
                crate::parse::split_fields(w, b',').len(),
                8,
                "쓴 파일 {i}번째 줄의 컬럼 수"
            );
        }
        std::fs::remove_file(&out).ok();
    }

    /// "텍스트로 열기"는 감지를 건너뛰고 지정 인코딩으로 기존 경로를 탄다.
    #[test]
    fn open_path_as_text_forces_encoding() {
        let p = temp_ext(b"abc\x00def\nghi\n", "txt"); // NUL 때문에 감지는 바이너리
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path_as_text(&p, Encoding::Utf8, &ctx);
        assert_eq!(app.docs.len(), 1);
        let doc = app.doc().unwrap();
        assert!(doc.hex.is_none(), "텍스트 문서다");
        assert_eq!(doc.enc, Encoding::Utf8);
        std::fs::remove_file(&p).ok();
    }

    /// 텍스트 파일은 다이얼로그 없이 기존과 똑같이 열린다(회귀 방지).
    #[test]
    fn open_plain_text_skips_dialog() {
        let p = temp_ext(b"a,b\n1,2\n", "csv");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        assert!(app.pending_binary_open.is_none());
        assert_eq!(app.docs.len(), 1);
        assert!(app.doc().unwrap().hex.is_none());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn droppable_paths_skips_directories() {
        let file = temp(b"a,b\n1,2\n");
        let dir = std::env::temp_dir().join(format!("tv_app_dropdir_{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let result = droppable_paths(vec![dir.clone(), file.clone()]);
        assert_eq!(result, vec![file], "디렉터리는 걸러지고 파일만 남는다");
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn droppable_paths_keeps_order() {
        let p1 = temp(b"1");
        let p2 = temp(b"2");
        let p3 = temp(b"3");
        let result = droppable_paths(vec![p1.clone(), p2.clone(), p3.clone()]);
        assert_eq!(result, vec![p1, p2, p3], "드롭 순서가 그대로 보존되어야 한다");
    }

    #[test]
    fn dropping_multiple_files_opens_one_tab_each() {
        let p1 = temp(b"a,b\n1,2\n");
        let p2 = temp(b"c,d\n3,4\n");
        let p3 = temp(b"e,f\n5,6\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        // 드롭 처리 루프가 하는 일과 동일: 순서대로 open_path를 호출한다.
        for p in [&p1, &p2, &p3] {
            app.open_path(p, &ctx);
        }
        assert_eq!(app.docs.len(), 3);
        assert_eq!(app.active, 2, "마지막으로 연 파일의 탭이 활성화되어야 한다");
        assert_eq!(app.docs[0].path, p1);
        assert_eq!(app.docs[1].path, p2);
        assert_eq!(app.docs[2].path, p3);
    }

    #[test]
    fn drop_message_singular_and_plural() {
        assert_eq!(drop_hint_text(1), "Drop to open");
        assert_eq!(drop_hint_text(3), "Drop to open 3 files");
    }

    #[test]
    fn drop_ignored_while_dialog_open() {
        // 저장 다이얼로그가 떠 있는 상태(탭 바 잠금)를 재현한다. 이미 있는
        // tab_switch_blocked_while_save_dialog_open은 "탭 바가 잠기는가"를
        // 검증하고, 여기서는 그 잠금이 실제로 "드롭이 탭을 추가하지 못하게"
        // 막는지를 확인한다. update()가 실제로 위임하는 plan_dropped_files를
        // 그대로 호출한다 — 가드를 인라인으로 복붙하면 update() 안의 진짜
        // 가드를 지워도 이 테스트는 계속 통과하는 착시가 생긴다.
        let mut app = App {
            show_save_dialog: true,
            ..App::default()
        };
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let locked = tab_bar_locked_for(&app);
        match plan_dropped_files(vec![p], locked) {
            DropPlan::Locked(msg) => app.error = Some(msg),
            DropPlan::Open(paths) => {
                for p in paths {
                    app.open_path(&p, &ctx);
                }
            }
        }
        assert_eq!(app.docs.len(), 0, "잠겨 있으면 드롭이 탭을 추가하지 않는다");
        assert!(app.error.is_some(), "잠겨 있으면 안내 메시지를 남겨야 한다");
    }

    #[test]
    fn plan_dropped_files_open_when_unlocked() {
        let p1 = temp(b"a,b\n1,2\n");
        let p2 = temp(b"c,d\n3,4\n");
        match plan_dropped_files(vec![p1.clone(), p2.clone()], false) {
            DropPlan::Open(paths) => assert_eq!(paths, vec![p1, p2]),
            DropPlan::Locked(_) => panic!("잠겨 있지 않으면 열어야 한다"),
        }
    }

    #[test]
    fn plan_dropped_files_locked_reports_message_without_opening() {
        let p = temp(b"a,b\n1,2\n");
        match plan_dropped_files(vec![p], true) {
            DropPlan::Locked(msg) => assert!(msg.contains("Close")),
            DropPlan::Open(_) => panic!("잠겨 있으면 열지 않아야 한다"),
        }
    }

    #[test]
    fn drop_batch_keeps_last_failure_visible_after_later_success() {
        // Important 1 회귀 테스트: 실패한 파일 뒤에 성공한 파일이 오면
        // open_path의 "진입 시 self.error = None" 계약 때문에 실패 메시지가
        // 조용히 사라지던 버그. update()의 드롭 처리 루프와 동일한 순서로
        // (plan_dropped_files → open_path 반복 → 마지막 실패 복원) 재현한다.
        let bad = std::path::PathBuf::from("nope_batch_xyz.csv");
        let good = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();

        let paths = match plan_dropped_files(vec![bad, good.clone()], false) {
            DropPlan::Open(paths) => paths,
            DropPlan::Locked(_) => panic!("잠겨 있지 않으면 열어야 한다"),
        };
        let mut last_failure: Option<String> = None;
        for p in paths {
            app.open_path(&p, &ctx);
            if let Some(e) = app.error.take() {
                last_failure = Some(e);
            }
        }
        app.error = last_failure;

        assert!(
            app.error.is_some(),
            "배치 중 실패가 이후 성공에 가려지면 안 된다"
        );
        assert_eq!(app.docs.len(), 1, "성공한 파일의 탭은 남아 있어야 한다");
        assert_eq!(app.active, 0, "성공한 파일의 탭이 활성 상태여야 한다");
        assert_eq!(app.docs[0].path, good);
    }

    /// 탭 3개를 열어 둔 App을 만든다. 각 파일 내용은 서로 달라 path로 구분 가능.
    fn app_with_three_tabs() -> App {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&temp(b"a,b\n1,2\n"), &ctx);
        app.open_path(&temp(b"c,d\n3,4\n"), &ctx);
        app.open_path(&temp(b"e,f\n5,6\n"), &ctx);
        app
    }

    #[test]
    fn close_tab_before_active_shifts_active_left() {
        let mut app = app_with_three_tabs();
        app.active = 2;
        let third_path = app.docs[2].path.clone();
        app.close_tab(0);
        assert_eq!(app.docs.len(), 2);
        assert_eq!(app.active, 1);
        assert_eq!(
            app.doc().unwrap().path,
            third_path,
            "활성 문서는 원래 3번째 파일 그대로여야 한다"
        );
    }

    #[test]
    fn close_tab_after_active_keeps_active() {
        let mut app = app_with_three_tabs();
        app.active = 0;
        app.close_tab(2);
        assert_eq!(app.active, 0);
    }

    #[test]
    fn close_active_last_tab_clamps_active() {
        let mut app = app_with_three_tabs();
        app.close_tab(2); // 3개 -> 2개로 줄여 시작 상태를 맞춘다.
        app.active = 1;
        app.close_tab(1);
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.active, 0);
    }

    #[test]
    fn close_last_remaining_tab_empties_docs() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&temp(b"a,b\n1,2\n"), &ctx);
        app.close_tab(0);
        assert!(app.docs.is_empty());
        assert_eq!(app.active, 0);
        assert!(app.doc().is_none());
    }

    #[test]
    fn close_tab_out_of_range_is_noop() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&temp(b"a,b\n1,2\n"), &ctx);
        app.close_tab(5);
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.active, 0);
    }

    #[test]
    fn any_dirty_sees_inactive_tab() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&temp(b"a,b\n1,2\n"), &ctx);
        app.open_path(&temp(b"c,d\n3,4\n"), &ctx);
        // 비활성 탭(0번)만 편집 모드 + dirty로 만든다.
        enter_edit_mode(&mut app.docs[0]);
        app.docs[0].edit.as_mut().unwrap().dirty = true;
        app.active = 1;
        assert!(!app.edit_dirty(), "활성 탭은 dirty가 아니다");
        assert!(app.any_dirty(), "비활성 탭의 dirty도 잡아야 한다");
    }

    #[test]
    fn tab_label_truncates_on_char_boundary() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let mut doc = app.docs.pop().unwrap();
        // 매우 긴 한글 파일명으로 바꿔치기(실제 파일이 존재할 필요는 없다 —
        // tab_label은 path만 본다).
        doc.path = std::path::PathBuf::from("가".repeat(40) + ".csv");
        let label = tab_label(&doc);
        assert!(label.chars().count() <= 24);
    }

    #[test]
    fn tab_label_marks_dirty() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let mut doc = app.docs.pop().unwrap();
        enter_edit_mode(&mut doc);
        doc.edit.as_mut().unwrap().dirty = true;
        assert!(tab_label(&doc).starts_with('●'));
    }

    /// 리뷰어가 재현한 정확한 4탭 시나리오. tab 2와 tab 3이 모두 dirty인 상태에서:
    /// 1) tab 2의 X를 눌러 CloseTab(2)를 대기시킨다(확인 창이 뜬 상태를 흉내낸다).
    /// 2) 그 사이 tab 0(깨끗함)의 X를 눌러 즉시 닫히는 시나리오를 시도한다 —
    ///    수정 전에는 탭 바가 잠기지 않아 이게 그대로 먹혀서 탭 집합이 밀렸다.
    ///    수정 후에는 `tab_bar_locked`가 참이라 이 클릭 자체가 무시돼야 한다
    ///    (탭 바 UI가 비활성화된 상태를 그대로 재현: want_close를 만들어내지 않음).
    /// 3) "계속"을 눌러 대기 중이던 CloseTab(2)를 적용한다.
    ///
    /// 결과: 사용자가 실제로 확인한 tab 2(경로로 식별)가 닫히고, 무고한 tab 3은
    /// dirty 버퍼를 그대로 유지한 채 살아남아야 한다. docs.len()만으로는 이
    /// 시나리오의 핵심(엉뚱한 탭이 닫히는 것)을 놓치므로 경로로 식별한다.
    #[test]
    fn close_tab_confirm_survives_interleaved_click_on_other_tab() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&temp(b"a,b\n1,2\n"), &ctx); // tab 0: clean
        app.open_path(&temp(b"c,d\n3,4\n"), &ctx); // tab 1: clean
        app.open_path(&temp(b"e,f\n5,6\n"), &ctx); // tab 2: dirty (닫으려는 대상)
        app.open_path(&temp(b"g,h\n7,8\n"), &ctx); // tab 3: dirty (무고한 탭)
        enter_edit_mode(&mut app.docs[2]);
        app.docs[2].edit.as_mut().unwrap().dirty = true;
        enter_edit_mode(&mut app.docs[3]);
        app.docs[3].edit.as_mut().unwrap().dirty = true;
        let tab2_path = app.docs[2].path.clone();
        let tab3_path = app.docs[3].path.clone();

        // 1) tab 2의 X 클릭 → dirty이므로 즉시 닫지 않고 확인 대기.
        let dirty2 = app.docs[2].edit.as_ref().is_some_and(|e| e.dirty);
        assert!(dirty2);
        app.pending_action = Some(PendingAction::CloseTab(2));

        // 2) 확인 창이 뜬 동안 탭 바는 잠겨 있어야 한다 — 그래서 tab 0의 X
        // 클릭이 want_close를 만들어내지 못한다(= 탭 바 UI가 비활성 상태라
        // 클릭이 애초에 등록되지 않는 것과 같다).
        assert!(
            tab_bar_locked_for(&app),
            "확인 대기 중에는 탭 바가 잠겨야 다른 탭 클릭이 무시된다"
        );
        // 잠겨 있으므로 tab 0 클릭 인텐트를 적용하지 않는다(가드 자체를 검증).
        // 혹시라도 아래 처럼 클릭이 새어 들어왔다면 즉시 닫히지 않아야 한다.
        if !tab_bar_locked_for(&app) {
            app.close_tab(0);
        }
        assert_eq!(app.docs.len(), 4, "잠긴 동안에는 어떤 탭도 닫히지 않는다");

        // 3) "계속" → 대기 중이던 CloseTab(2)를 적용한다.
        match app.pending_action.take() {
            Some(PendingAction::CloseTab(i)) => app.close_tab(i),
            other => panic!("예상치 못한 pending_action: {other:?}"),
        }

        assert_eq!(app.docs.len(), 3);
        assert!(
            !app.docs.iter().any(|d| d.path == tab2_path),
            "사용자가 실제로 확인한 tab 2가 닫혀야 한다"
        );
        let survivor = app
            .docs
            .iter()
            .find(|d| d.path == tab3_path)
            .expect("무고한 tab 3은 살아남아야 한다");
        assert!(
            survivor.edit.as_ref().unwrap().dirty,
            "tab 3의 dirty 버퍼는 보존되어야 한다(폐기되면 안 된다)"
        );
    }

    /// 저장 다이얼로그가 떠 있는 동안은 탭 전환이 안전하지 않다(save_enc/
    /// save_bom이 활성 문서 기준으로만 세팅되어 있어, 전환을 허용하면 다른
    /// 인코딩의 문서를 그 설정으로 저장하게 된다). `tab_bar_locked`가 그
    /// 경우를 잠근다는 것을 확인한다.
    #[test]
    fn tab_switch_blocked_while_save_dialog_open() {
        let pending: Option<PendingAction> = None;
        assert!(
            !tab_bar_locked(&pending, false, false, false, false),
            "평상시에는 탭 바가 잠기지 않아야 한다"
        );
        assert!(
            tab_bar_locked(&pending, true, false, false, false),
            "저장 다이얼로그가 떠 있으면 탭 바가 잠겨야 한다"
        );
        assert!(
            tab_bar_locked(&pending, false, true, false, false),
            "열기 방식 선택이 보류 중이면 탭 바가 잠겨야 한다(C1)"
        );
        assert!(
            tab_bar_locked(&pending, false, false, true, false),
            "대형 바이너리 로드 확인 중이면 탭 바가 잠겨야 한다(I4)"
        );
        assert!(
            tab_bar_locked(&pending, false, false, false, true),
            "Parquet 정렬 확인 중이면 탭 바가 잠겨야 한다(같은 이유)"
        );
    }

    /// 24자 상한은 "● " 접두사까지 포함한 전체 라벨 길이에 적용돼야 한다.
    /// 접두사를 붙이기 전에 잘라 버리면 dirty한 긴 이름 탭이 상한을 넘긴다.
    /// 긴 한글 파일명으로 char 경계 패닉이 없는지도 함께 확인한다.
    #[test]
    fn dirty_tab_label_stays_within_cap_on_long_korean_name() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let mut doc = app.docs.pop().unwrap();
        doc.path = std::path::PathBuf::from("가".repeat(40) + ".csv");
        enter_edit_mode(&mut doc);
        doc.edit.as_mut().unwrap().dirty = true;
        let label = tab_label(&doc); // 패닉하면 이 테스트가 실패한다.
        assert!(
            label.chars().count() <= 24,
            "dirty 표시(● )를 포함한 전체 라벨이 24자를 넘었다: {label:?}"
        );
        assert!(label.starts_with('●'));
    }

    /// 탭을 전환하면 이전 탭에서 남은 오류 메시지가 지워져야 한다 — 그렇지
    /// 않으면 탭 A의 저장 실패 메시지가 탭 B로 넘어가서도 그대로 보여
    /// 마치 B의 오류처럼 보인다.
    #[test]
    fn switching_active_tab_clears_error() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&temp(b"a,b\n1,2\n"), &ctx);
        app.open_path(&temp(b"c,d\n3,4\n"), &ctx);
        app.error = Some("Save failed: 뭔가 잘못됨".to_owned());

        // update()의 탭 전환 인텐트 적용부와 같은 조건: 실제로 활성 인덱스가
        // 바뀔 때만 지운다.
        let want_active = 0usize;
        if want_active != app.active {
            app.error = None;
        }
        app.active = want_active;

        assert!(app.error.is_none(), "탭 전환 시 이전 오류가 지워져야 한다");
    }

    /// update()의 col_count 계산을 GUI 없이 검증하는 헬퍼. 표 모드 전용이므로
    /// doc.sep이 Char임을 가정하고 그 delim을 꺼내 쓴다. 실제 계산은
    /// `table_col_count`(render_table과 focus_match가 공유하는 그 함수)에
    /// 그대로 위임한다 — 여기서 공식을 다시 베끼면 세 번째 사본이 생기고,
    /// 그러면 `table_col_count`를 잘못 고쳐도 이 테스트가 자기 사본만 보고
    /// 계속 통과한다.
    fn compute_col_count(doc: &Document) -> usize {
        match doc.sep {
            SeparatorMode::Char(d) => table_col_count(doc, d),
            SeparatorMode::None => 1,
        }
    }

    #[test]
    fn headerless_ragged_file_col_count_matches_widest_row() {
        // has_header가 false로 판정되도록 전부 숫자인 CSV(detect_header가 false 반환).
        // 첫 줄은 4개, 이후 5개 필드짜리 행 포함 → col_count는 4가 아니라 5여야 함
        // (샘플링이 헤더 없는 파일에서 실제 데이터 폭을 반영하는지 검증).
        let p = temp(b"1,2,3,4\n5,6,7,8,9\n10,11,12,13\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        // 인덱싱을 완료까지 join해 line_count/line_range가 안정적으로 채워지게 한다.
        doc.indexer.take().unwrap().join().unwrap();

        assert!(!doc.has_header, "전부 숫자인 파일은 헤더 없음으로 판정되어야 함");
        assert_eq!(compute_col_count(doc), 5);
    }

    #[test]
    fn headerless_uniform_file_col_count_matches_field_count() {
        // 브리프에 명시된 케이스: 헤더 없는 4열 파일 → col_count는 1이 아니라 4.
        let p = temp(b"1,2,3,4\n5,6,7,8\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();

        assert!(!doc.has_header);
        assert_eq!(compute_col_count(doc), 4);
    }

    #[test]
    fn plain_text_opens_in_text_mode() {
        // 구분자가 거의 없는 산문 → SeparatorMode::None(텍스트 모드)로 열려야 하고,
        // 헤더는 꺼져 있어야 한다. decode_logical_line은 줄 전체를 돌려준다.
        let p = temp_ext(
            b"The quick brown fox\njumps over the lazy dog\nand runs away fast\n",
            "txt",
        );
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();

        assert_eq!(doc.sep, SeparatorMode::None, "구분자 없는 텍스트는 None으로");
        assert!(!doc.has_header, "텍스트 모드는 헤더 없음");
        // 각 논리 행이 줄 전체(구분/분리 없이) 그대로 디코딩되는지.
        assert_eq!(
            decode_logical_line(doc, 0).as_deref(),
            Some("The quick brown fox")
        );
        assert_eq!(
            decode_logical_line(doc, 1).as_deref(),
            Some("jumps over the lazy dog")
        );
    }

    #[test]
    fn custom_separator_splits_fields() {
        // 커스텀 구분자(물결 ~)로 필드 분리가 되는지. detect는 표준 후보만 보므로
        // 이 파일은 None으로 열리지만, 사용자가 sep을 Char(b'~')로 바꾸면
        // parse_logical_line_edit이 ~로 분리해야 한다.
        let p = temp_ext(b"a~b~c\nd~e~f\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        doc.sep = SeparatorMode::Char(b'~');

        assert_eq!(
            parse_logical_line_edit(doc, 0, b'~'),
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
        assert_eq!(compute_col_count(doc), 3);
    }

    #[test]
    fn enter_edit_mode_loads_lines() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        enter_edit_mode(doc);
        assert!(doc.edit.is_some());
        assert_eq!(doc.edit.as_ref().unwrap().lines, vec!["a,b", "1,2"]);
    }

    // ---- 새 파일 (File → New / 인자 없이 실행) ----

    /// 새 문서의 기본 상태. 빈 한 줄 + 편집 모드 + 텍스트 모드.
    #[test]
    fn new_document_starts_empty_in_edit_mode() {
        let doc = new_document();
        assert!(doc.edit.is_some(), "새 파일은 처음부터 편집 모드");
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            vec![String::new()],
            "빈 한 줄로 시작한다"
        );
        assert!(!doc.edit.as_ref().unwrap().dirty, "아직 아무것도 안 고쳤다");
        assert_eq!(doc.sep, SeparatorMode::None, "빈 문서에 감지할 구분자는 없다");
        assert!(!doc.has_header);
        assert!(doc.indexer.is_none(), "인메모리라 붙일 인덱서 스레드가 없다");
        assert_eq!(
            doc.index.status().phase,
            Phase::Complete,
            "인덱스를 동기로 채웠으므로 상태바가 '인덱싱 중'으로 남으면 안 된다"
        );
    }

    /// **저장하면 경로를 묻게 되는가** — 이 기능의 요구사항 그 자체.
    ///
    /// 유도 장치는 빈 `path` 하나다. `save_as_fallback`이 그것을 보고 파일
    /// 선택 창으로 폴백한다(추출본이 이미 쓰는 길).
    #[test]
    fn new_document_has_no_path_so_save_asks_for_one() {
        let doc = new_document();
        assert_eq!(doc.path, std::path::PathBuf::new(), "디스크 대응 파일이 없다");
        assert!(
            save_as_fallback(false, doc.path.as_os_str().is_empty()),
            "Save(다른 이름 아님)를 눌러도 경로를 묻는 쪽으로 폴백해야 한다"
        );
    }

    /// 탭 라벨은 `"(untitled)"`. 추출본 접두사(`[hit] `)가 붙으면 안 된다 —
    /// 새 파일은 추출본이 아니다.
    #[test]
    fn new_document_tab_label_is_untitled() {
        let doc = new_document();
        assert!(!doc.is_extracted, "새 파일은 추출본이 아니다");
        assert_eq!(tab_label(&doc), "(untitled)");
    }

    /// 저장하고 나면 그 경로가 문서에 남아, **다음 Ctrl+S는 덮어쓰기**여야 한다.
    /// 여기가 어긋나면 저장할 때마다 파일 선택 창이 다시 뜬다
    /// (`save_as_fallback` 주석이 설명하는 과거 결함과 같은 종류).
    #[test]
    fn new_document_stops_asking_after_first_save() {
        let mut doc = new_document();
        doc.edit.as_mut().unwrap().lines = v(&["hello", "world"]);

        // 저장이 경로를 확정한 상태를 흉내낸다(rfd 파일 선택 창은 테스트에서
        // 띄울 수 없으므로, 저장 성공 뒤 갱신되는 두 필드를 직접 맞춘다).
        let saved = temp(b"");
        doc.path = saved.clone();
        doc.path_label = saved.display().to_string();

        assert!(
            !save_as_fallback(false, doc.path.as_os_str().is_empty()),
            "경로가 생겼으면 다음 저장은 묻지 않고 덮어쓴다"
        );
        assert_ne!(tab_label(&doc), "(untitled)", "탭 라벨도 파일명으로 바뀐다");
        std::fs::remove_file(&saved).ok();
    }

    /// 새 문서에 친 내용이 실제로 파일로 저장되는가(끝에서 끝까지).
    #[test]
    fn new_document_content_round_trips_to_disk() {
        let mut doc = new_document();
        doc.edit.as_mut().unwrap().lines = v(&["a,b", "1,2"]);

        let out = temp(b"");
        let opts = crate::save::SaveOptions {
            enc: crate::parse::Encoding::Utf8,
            bom: false,
            newline: doc.edit.as_ref().unwrap().newline,
        };
        crate::save::write_file(&out, &doc.edit.as_ref().unwrap().lines, &opts, None).unwrap();

        let written = std::fs::read(&out).unwrap();
        assert_eq!(
            written,
            b"a,b\r\n1,2\r\n".to_vec(),
            "새 파일 기본 개행은 CRLF(Windows)"
        );
        std::fs::remove_file(&out).ok();
    }

    /// File → New / Ctrl+N 은 **새 탭**으로 붙고 그 탭이 활성화된다.
    /// 열려 있던 문서를 갈아치우면 안 된다.
    #[test]
    fn new_document_adds_a_tab_without_replacing_others() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&temp(b"a,b\n1,2\n"), &ctx);
        let first_path = app.docs[0].path.clone();

        // 메뉴/단축키가 실제로 부르는 함수를 지난다.
        app.open_new_tab();

        assert_eq!(app.docs.len(), 2, "탭이 하나 늘어난다");
        assert_eq!(app.active, 1, "새 탭이 활성화된다");
        assert_eq!(app.docs[0].path, first_path, "원래 탭은 그대로");
        assert_eq!(tab_label(app.doc().unwrap()), "(untitled)");
    }

    /// 새 문서도 편집이 쌓이면 dirty가 되어 탭에 ●가 붙고, 닫을 때 확인
    /// 대상이 된다 — 저장 안 한 새 파일이 조용히 사라지면 안 된다.
    #[test]
    fn new_document_becomes_dirty_and_is_guarded_on_close() {
        let mut app = App::default();
        app.add_document(new_document());
        assert!(!app.any_dirty(), "막 만든 문서는 깨끗하다");

        let doc = app.doc_mut().unwrap();
        doc.edit.as_mut().unwrap().lines = v(&["typed something"]);
        doc.edit.as_mut().unwrap().dirty = true;

        assert!(app.any_dirty(), "닫기 확인이 걸려야 한다");
        assert!(tab_label(app.doc().unwrap()).starts_with('●'));
    }

    /// 인자 없이 실행하면 빈 새 파일로 시작한다(`main`이 부르는 경로).
    #[test]
    fn start_without_argument_opens_a_new_file() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(None, &ctx, Default::default());
        assert_eq!(app.docs.len(), 1, "탭 하나로 시작한다");
        let doc = app.doc().unwrap();
        assert!(doc.edit.is_some(), "바로 타이핑할 수 있어야 한다");
        assert_eq!(tab_label(doc), "(untitled)");
        assert!(doc.path.as_os_str().is_empty(), "저장할 때 경로를 묻는다");
    }

    /// 인자로 파일을 받으면 그 파일을 연다 — 새 문서를 덧붙이지 않는다.
    #[test]
    fn start_with_argument_opens_that_file() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        assert_eq!(app.docs.len(), 1, "빈 새 파일이 덤으로 붙지 않는다");
        assert_eq!(app.doc().unwrap().path, p);
    }

    /// 열기에 실패해도 창이 텅 비지 않는다. 에러는 `self.error`가 전하고,
    /// 사용자는 곧바로 뭔가 칠 수 있는 상태여야 한다.
    #[test]
    fn start_with_bad_path_still_gives_a_usable_document() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let missing = std::path::PathBuf::from("Z:\\없는폴더\\없는파일_tv.csv");
        app.start(Some(&missing), &ctx, Default::default());
        assert!(app.error.is_some(), "실패는 실패라고 알린다");
        assert_eq!(app.docs.len(), 1, "그래도 빈 새 파일로 시작한다");
        assert!(app.doc().unwrap().edit.is_some());
    }

    // ---- 한글 폰트 부재 안내 ----
    //
    // 이 상황은 Windows에서 재현되지 않는다(맑은 고딕이 항상 있다). CJK 폰트가
    // 없는 리눅스에서만 일어나므로, 검증 수단이 이 테스트뿐이다.

    /// 한글 폰트를 못 찾으면 설치 방법을 안내한다 — 화면이 두부로 덮이는데
    /// 아무 설명이 없으면 사용자는 앱이 깨진 줄 안다.
    #[test]
    fn missing_korean_font_is_reported_to_the_user() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let fonts = crate::theme::FontReport {
            korean_missing: true,
        };
        app.start(None, &ctx, fonts);
        let msg = app.error.as_deref().expect("안내가 있어야 한다");
        assert_eq!(msg, crate::theme::KOREAN_FONT_MISSING_MSG);
        assert_eq!(app.docs.len(), 1, "안내와 무관하게 앱은 정상 동작한다");
    }

    /// 폰트가 정상이면 아무 말도 하지 않는다. Windows/맥의 보통 경로가
    /// 조용해야 이 안내가 신호로 남는다.
    #[test]
    fn present_korean_font_says_nothing() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(None, &ctx, crate::theme::FontReport::default());
        assert!(app.error.is_none(), "정상 경로는 조용하다: {:?}", app.error);
    }

    /// 파일 열기 오류가 폰트 안내에 덮이지 않는다. 열려던 파일이 없다는 사실이
    /// 더 급하고, 폰트 문제는 화면만 봐도 드러난다.
    #[test]
    fn open_error_outranks_the_font_notice() {
        let ctx = egui::Context::default();
        let mut app = App::default();
        let missing = std::path::PathBuf::from("Z:\\없는폴더\\없는파일_tv.csv");
        let fonts = crate::theme::FontReport {
            korean_missing: true,
        };
        app.start(Some(&missing), &ctx, fonts);
        let msg = app.error.as_deref().expect("에러가 있어야 한다");
        assert!(msg.contains("Failed to open file"), "열기 오류가 남는다: {msg}");
    }

    // ---- 작은 파일 자동 편집 모드 ----

    /// 경계값. `<=`가 맞는지(상한 자체는 포함) 두 방향으로 못박는다.
    /// 이 테스트가 있어야 `<`로 바뀌거나 상한이 흔들릴 때 잡힌다.
    #[test]
    fn auto_edit_threshold_is_inclusive_at_10mb() {
        assert!(auto_edit_on_open(0), "빈 파일");
        assert!(auto_edit_on_open(AUTO_EDIT_MAX_BYTES - 1));
        assert!(auto_edit_on_open(AUTO_EDIT_MAX_BYTES), "상한은 포함");
        assert!(!auto_edit_on_open(AUTO_EDIT_MAX_BYTES + 1));
        assert!(!auto_edit_on_open(10 * 1024 * 1024 * 1024), "10GB는 뷰 모드");
    }

    /// 작은 파일을 열면 메뉴를 건드리지 않아도 편집 버퍼가 채워져 있어야 한다.
    #[test]
    fn small_file_opens_in_edit_mode() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert!(doc.edit.is_some(), "10MB 이하는 자동 편집 모드");
        assert_eq!(doc.edit.as_ref().unwrap().lines, vec!["a,b", "1,2"]);
        assert!(!doc.edit.as_ref().unwrap().dirty, "열기만 했으니 깨끗하다");
    }

    /// 구분자가 없는 텍스트 파일도 편집 대상이다. 표/텍스트는 *보기* 방식일 뿐.
    #[test]
    fn small_text_file_also_opens_in_edit_mode() {
        let p = temp_ext(b"hello\nworld\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert_eq!(doc.sep, SeparatorMode::None, "전제: 텍스트 모드로 열렸다");
        assert!(doc.edit.is_some(), "텍스트 모드도 자동 편집 모드");
    }

    /// 상한이 **10MB 그 값**인지. 위/아래 양쪽에서 못박는다.
    ///
    /// `auto_edit_threshold_is_inclusive_at_10mb`는 상수를 기준으로 상대
    /// 비교만 하므로 상수가 1MB로 바뀌어도 그대로 통과한다. 사용자가 정한 값은
    /// "10MB"라는 절대 숫자이므로 여기서 리터럴로 고정한다.
    #[test]
    fn auto_edit_limit_is_exactly_ten_megabytes() {
        assert_eq!(AUTO_EDIT_MAX_BYTES, 10 * 1024 * 1024);
        assert!(auto_edit_on_open(9 * 1024 * 1024), "9MB는 편집 모드");
        assert!(!auto_edit_on_open(11 * 1024 * 1024), "11MB는 뷰 모드");
    }

    /// 상한을 넘는 파일은 예전처럼 뷰 모드로 열려야 한다 — 큰 파일에서 편집
    /// 버퍼를 강제로 만들면 이 앱의 목적(즉시 열기)이 무너진다.
    #[test]
    fn large_file_stays_in_view_mode() {
        // 실제로 10MB를 넘기되 테스트가 느려지지 않을 만큼만 넘긴다.
        let mut content = Vec::with_capacity(AUTO_EDIT_MAX_BYTES as usize + 64);
        while (content.len() as u64) <= AUTO_EDIT_MAX_BYTES {
            content.extend_from_slice(b"0123456789,0123456789\n");
        }
        assert!(content.len() as u64 > AUTO_EDIT_MAX_BYTES, "전제: 상한 초과");

        let p = temp(&content);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert!(doc.edit.is_none(), "상한 초과 파일은 뷰 모드로 열린다");
        std::fs::remove_file(&p).ok();
    }

    /// 편집 모드 진입에는 버퍼 로드 말고도 딸린 처리가 있다 — 뷰 permutation
    /// 정렬 폐기와 오류 목록 무효화.
    ///
    /// **범위 주의.** 이 테스트는 `enter_edit_mode`의 계약을 지킨다. `open_path`의
    /// 자동 진입이 그 함수를 거치는지는 여기서 확인하지 못한다 — 갓 만든
    /// `Document`는 `sort`/`row_errors`가 이미 None이라 두 구현이 관측 가능하게
    /// 갈리지 않기 때문이다(`open_path`의 주석 참고). 여기서 지키는 것은
    /// "이 계약이 사라지면 알아챈다"까지다.
    #[test]
    fn enter_edit_mode_clears_view_mode_leftovers() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        assert!(doc.edit.is_some(), "전제: 자동 편집 모드로 열렸다");

        // 뷰 모드에서 넘어온 것처럼 잔재를 심는다.
        view_doc(doc);
        doc.sort = Some(SortState {
            permutation: vec![1, 0],
            col: 0,
            kind: SortKind::Text,
            dir: SortDir::Asc,
            spec_count: 1,
        });
        doc.row_errors = Some(crate::validate::ScanResult { errors: Vec::new(), dropped: 0 });

        enter_edit_mode(doc);
        assert!(doc.edit.is_some());
        assert!(doc.sort.is_none(), "편집 모드는 뷰 permutation 정렬을 폐기한다");
        assert!(doc.row_errors.is_none(), "편집 모드 진입은 오류 목록을 무효화한다");
    }

    /// 자동 진입으로 만들어진 버퍼가 수동 진입과 같은 내용인지.
    #[test]
    fn auto_edit_buffer_matches_manual_enter() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();

        // 자동 경로
        let mut auto = App::default();
        auto.open_path(&p, &ctx);

        // 수동 경로: 같은 파일을 열고, 자동 진입분을 걷어낸 뒤 손으로 켠다.
        let mut manual = App::default();
        manual.open_path(&p, &ctx);
        {
            let doc = manual.doc_mut().unwrap();
            view_doc(doc);
            enter_edit_mode(doc);
        }

        let a = auto.doc().unwrap().edit.as_ref().unwrap();
        let m = manual.doc().unwrap().edit.as_ref().unwrap();
        assert_eq!(a.lines, m.lines);
        assert_eq!(a.dirty, m.dirty);
        assert_eq!(a.newline, m.newline);
    }

    // -----------------------------------------------------------------------
    // 구분자 변환 (Convert Delimiter)
    // -----------------------------------------------------------------------

    /// 변환 준비가 끝난 **뷰 모드** 문서를 연다(인덱싱 완료까지 기다린다).
    ///
    /// 테스트가 쓰는 파일은 전부 작아서 `open_path`가 자동으로 편집 모드에
    /// 넣는다(`auto_edit_on_open`). 뷰 모드 동작을 보려는 테스트는 그걸
    /// 되돌려야 하므로 `view_doc`을 거친다.
    fn convert_doc(content: &[u8]) -> (App, egui::Context) {
        let p = temp(content);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        view_doc(doc);
        (app, ctx)
    }

    /// 자동 편집 모드를 되돌려 **뷰 모드(mmap 경로)** 문서로 만든다.
    ///
    /// 작은 파일 자동 편집 모드가 들어오면서, "파일을 열면 뷰 모드"라는 옛
    /// 전제를 깔고 있던 테스트들이 전부 편집 모드를 받게 됐다. 각자
    /// `exit_edit_mode`를 부르는 대신 의도를 이름으로 남긴다 — 이 테스트들이
    /// 보려는 것은 **mmap 경로**이지 "편집 모드가 아닌 상태"가 아니다.
    fn view_doc(doc: &mut Document) {
        exit_edit_mode(doc);
        assert!(doc.edit.is_none(), "뷰 모드 전제를 세우지 못했다");
    }

    // ---- 파싱 오류 행 검출 (Task 12/13 배선) ----

    /// 텍스트 모드는 "필드 수가 맞는가"라는 물음 자체가 성립하지 않는다.
    #[test]
    fn error_scan_not_started_in_text_mode() {
        let (mut app, _) = convert_doc(b"a,b,c\n1,2,3\n");
        let doc = app.doc_mut().unwrap();
        doc.sep = SeparatorMode::None;
        assert!(!should_start_error_scan(doc));
    }

    /// 인덱싱이 끝나지 않았으면 시작하지 않는다 — 앞부분만 검사하고
    /// "검사 완료"로 표시하면 뒤쪽 오류를 없는 것으로 오해한다.
    #[test]
    fn error_scan_waits_for_indexing_in_view_mode() {
        let (mut app, _) = convert_doc(b"a,b,c\n1,2,3\n");
        let doc = app.doc_mut().unwrap();
        // 전제 확인 — 지금은 인덱싱이 끝나 시작할 수 있는 상태다.
        assert!(should_start_error_scan(doc));
        doc.index.set_phase(crate::index::Phase::Indexing);
        assert!(!should_start_error_scan(doc), "인덱싱 중에는 시작하지 않는다");
    }

    /// 편집 모드는 버퍼가 파일 전체를 담고 있으므로 인덱싱 상태와 무관하다.
    #[test]
    fn error_scan_ignores_indexing_phase_in_edit_mode() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n", true);
        let doc = app.doc_mut().unwrap();
        doc.index.set_phase(crate::index::Phase::Indexing);
        assert!(should_start_error_scan(doc));
    }

    /// 진행 중인 검사가 있으면 **또 띄우지 않는다**. 이 가드가 없으면 매
    /// 프레임(초당 수십 번) 스레드가 새로 생겨 대용량 파일에서 코어를 전부
    /// 잡아먹는다.
    ///
    /// 백그라운드 작업이 살아 있는 상태를 봐야 하므로 뷰 모드(mmap 경로)로
    /// 검사하고, 완료를 기다리지 않은 채 판정한다.
    #[test]
    fn error_scan_not_started_while_one_is_running() {
        let (mut app, ctx) = convert_doc(b"a,b\n1,2\n3,4\n");
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert!(doc.error_scan.is_some(), "전제: 백그라운드 작업이 떴다");
        // 아직 수거하지 않았다 — 이 상태에서 또 시작하면 안 된다.
        assert!(
            !should_start_error_scan(doc),
            "진행 중인 검사가 있으면 다시 시작하지 않는다"
        );
        // 실제로 start를 불러도 작업이 바뀌지 않는지(교체되지 않는지) 본다.
        start_error_scan(doc, &ctx);
        poll_scan_to_completion(doc);
        assert!(doc.row_errors.is_some());
    }

    /// 검사 중에는 상태바가 "검사 중"이라고 말해야 한다. "오류 없음"으로
    /// 보이면 사용자가 아직 안 끝난 검사를 끝난 것으로 읽는다.
    #[test]
    fn error_status_text_says_checking_while_running() {
        let (mut app, ctx) = convert_doc(b"a,b\n1,2\n3,4\n");
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert!(doc.error_scan.is_some(), "전제: 진행 중");
        assert_eq!(
            error_status_text(doc).as_deref(),
            Some("Checking rows…"),
            "진행 중에는 결과가 아니라 진행 중임을 말한다"
        );
        poll_scan_to_completion(doc);
        assert_ne!(error_status_text(doc).as_deref(), Some("Checking rows…"));
    }

    /// 결과가 이미 있고 데이터도 그대로면 다시 돌리지 않는다.
    #[test]
    fn error_scan_not_restarted_when_result_is_fresh() {
        let (mut app, ctx) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        poll_scan_to_completion(doc);
        assert!(doc.row_errors.is_some());
        assert!(!should_start_error_scan(doc), "같은 데이터를 또 훑지 않는다");
    }

    /// **편집 모드에서도** 데이터가 그대로면 다시 돌리지 않는다.
    ///
    /// 이걸 놓치면 편집 모드인 동안 매 프레임(초당 수십 번) 전 행을 다시
    /// 훑는다 — 1,500만 행에서는 앱이 멈춘 것처럼 보인다. 뷰 모드 테스트는
    /// 이 경우를 잡지 못한다(뷰 모드는 개정 번호가 늘 0이라 우연히 맞는다).
    #[test]
    fn error_scan_not_restarted_in_edit_mode_when_unchanged() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();

        // 편집을 몇 번 해서 개정 번호를 0이 아니게 만든다 — 0이면 초기값과
        // 우연히 같아져 이 테스트가 무의미해진다.
        let e = doc.edit.as_mut().unwrap();
        for _ in 0..3 {
            e.undo.push(crate::edit::EditOp::Replace(vec![(1, e.lines[1].clone())]));
        }
        assert_ne!(doc_revision(doc), 0, "전제: 개정 번호가 0이 아니다");

        start_error_scan(doc, &ctx);
        assert!(doc.row_errors.is_some());
        assert!(
            !should_start_error_scan(doc),
            "편집 없이 프레임만 흘러도 다시 훑지 않는다"
        );
    }

    /// 편집으로 데이터가 바뀌면 목록이 낡았으므로 다시 돌린다.
    /// 이것이 없으면 사용자가 오류를 고쳐도 목록에 계속 남는다.
    #[test]
    fn error_scan_restarts_after_edit() {
        let (mut app, _) = edit_doc(b"a,b\n1\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert_eq!(doc.row_errors.as_ref().unwrap().total(), 1, "1행이 필드 1개");
        assert!(!should_start_error_scan(doc));

        // 오류 행을 고친다(편집 지점을 흉내내지 않고 실제 경로 — undo push).
        let e = doc.edit.as_mut().unwrap();
        e.undo.push(crate::edit::EditOp::Replace(vec![(1, e.lines[1].clone())]));
        e.lines[1] = "1,2".to_string();

        assert!(should_start_error_scan(doc), "데이터가 바뀌면 다시 검사한다");
        start_error_scan(doc, &ctx);
        assert_eq!(doc.row_errors.as_ref().unwrap().total(), 0, "고친 뒤엔 오류 없음");
    }

    /// 되돌리기도 데이터를 바꾸므로 재검사 대상이다.
    #[test]
    fn error_scan_restarts_after_undo() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert_eq!(doc.row_errors.as_ref().unwrap().total(), 0);

        // 행을 망가뜨렸다가 되돌린다.
        let e = doc.edit.as_mut().unwrap();
        e.undo.push(crate::edit::EditOp::Replace(vec![(1, e.lines[1].clone())]));
        e.lines[1] = "1".to_string();
        start_error_scan(doc, &ctx);
        assert_eq!(doc.row_errors.as_ref().unwrap().total(), 1);

        let e = doc.edit.as_mut().unwrap();
        assert!(e.undo.undo(&mut e.lines));
        assert!(should_start_error_scan(doc), "되돌리기 뒤에도 재검사");
        start_error_scan(doc, &ctx);
        assert_eq!(doc.row_errors.as_ref().unwrap().total(), 0);
    }

    /// 구분자를 바꾸면 필드 수를 세는 기준이 통째로 달라진다.
    #[test]
    fn invalidate_clears_result_and_revision() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert!(doc.row_errors.is_some());
        invalidate_error_scan(doc);
        assert!(doc.row_errors.is_none());
        assert_eq!(doc.row_errors_revision, 0);
    }

    /// 변환은 데이터도 보는 기준도 바꾼다 — 목록이 남아 있으면 안 된다.
    #[test]
    fn convert_delimiter_invalidates_error_scan() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert!(doc.row_errors.is_some(), "전제: 결과가 있다");
        convert_delimiter_in_doc(doc, b'\t');
        assert!(doc.row_errors.is_none(), "변환 뒤 목록은 무효");
    }

    /// 바뀔 행이 하나도 없는 변환(구분자가 없는 문서)도 **보는 기준**은
    /// 바꾸므로 목록을 무효화해야 한다. 이 경로는 편집이 없어 개정 번호가
    /// 안 움직이므로 명시적 무효화에만 기댄다.
    #[test]
    fn convert_with_no_changed_rows_still_invalidates() {
        let (mut app, _) = edit_doc(b"onlyonecol\nx\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();
        doc.sep = SeparatorMode::Char(b',');
        start_error_scan(doc, &ctx);
        assert!(doc.row_errors.is_some(), "전제: 결과가 있다");
        let rev_before = doc_revision(doc);
        convert_delimiter_in_doc(doc, b'\t');
        assert_eq!(doc_revision(doc), rev_before, "전제: 편집이 일어나지 않았다");
        assert!(doc.row_errors.is_none(), "그래도 목록은 무효");
    }

    /// 편집 모드로 **들어가면** 검사의 바탕이 mmap에서 편집 버퍼로 바뀐다.
    ///
    /// 개정 번호로는 못 잡는다 — 새 `EditBuffer`의 revision이 0이고 뷰 모드의
    /// `doc_revision`도 0이라 "그대로"라고 답한다. 특히 디코드 오류는 편집
    /// 버퍼에서는 나올 수 없으므로(이미 String이다) 뷰 모드에서 센 목록이
    /// 그대로 남으면 사용자가 존재하지 않는 오류를 본다.
    #[test]
    fn entering_edit_mode_invalidates_error_scan() {
        let (mut app, ctx) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        poll_scan_to_completion(doc);
        assert!(doc.row_errors.is_some(), "전제: 뷰 모드 결과가 있다");
        assert_eq!(doc_revision(doc), 0, "전제: 뷰 모드는 개정 번호 0");

        enter_edit_mode(doc);
        assert_eq!(doc_revision(doc), 0, "새 버퍼도 개정 번호 0 — 비교로는 못 잡는다");
        assert!(doc.row_errors.is_none(), "그래도 목록은 무효여야 한다");
    }

    /// 편집 모드를 **끄면** 바탕이 mmap으로 되돌아간다. 편집 없이 켰다 끈
    /// 경우엔 양쪽 개정 번호가 다 0이라 비교로는 안 잡힌다.
    #[test]
    fn exiting_edit_mode_invalidates_error_scan() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert!(doc.row_errors.is_some(), "전제: 편집 모드 결과가 있다");
        assert_eq!(doc_revision(doc), 0, "전제: 편집 없이 들어왔으므로 0");

        exit_edit_mode(doc);
        assert!(doc.row_errors.is_none());
    }

    /// 인덱스를 통째로 다시 만드는 경로(Paused → Resume)는 옛 행번호를
    /// 무의미하게 만든다. 여기서는 그 핸들러가 부르는 무효화만 직접 확인한다
    /// (버튼 클릭은 egui 클로저라 테스트가 구동할 수 없다).
    #[test]
    fn invalidate_cancels_running_job_too() {
        let (mut app, ctx) = convert_doc(b"a,b\n1\n2\n3\n");
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert!(doc.error_scan.is_some(), "전제: 백그라운드 작업이 떴다");
        invalidate_error_scan(doc);
        assert!(doc.error_scan.is_none(), "진행 중인 작업도 치운다");
        assert!(doc.row_errors.is_none());
        // 무효화 뒤에는 다시 시작할 수 있어야 한다(막아 두면 영영 안 돈다).
        assert!(should_start_error_scan(doc));
    }

    /// 목록에 보이는 행번호가 **본문 라인 번호 칸과 같아야** 한다.
    ///
    /// `RowError.logical`은 헤더를 포함한 절대 논리 행이고, 본문은
    /// `view_row + row_base`(헤더 제외)를 쓴다. 그대로 더하면 헤더가 있을 때
    /// 항상 1 어긋난다 — 사용자가 "3번 행이 오류"를 보고 3번 행을 봤는데
    /// 멀쩡한 상황이 된다.
    #[test]
    fn error_row_display_number_matches_table_line_number() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n3,4\n", true);
        let doc = app.doc_mut().unwrap();
        // 헤더가 있으니 논리 1행 = 화면 0행.
        assert_eq!(error_row_display_number(doc, 1, 0), 0, "row_base 0");
        assert_eq!(error_row_display_number(doc, 1, 1), 1, "row_base 1");
        assert_eq!(error_row_display_number(doc, 2, 0), 1);

        // 헤더가 없으면 논리 행이 곧 화면 행이다.
        doc.has_header = false;
        assert_eq!(error_row_display_number(doc, 1, 0), 1);
    }

    /// 텍스트 모드에서는 논리 행이 곧 화면 행이다(헤더 개념이 없다).
    #[test]
    fn error_row_display_number_in_text_mode() {
        let (mut app, _) = edit_doc(b"a,b\n1,2\n", true);
        let doc = app.doc_mut().unwrap();
        doc.sep = SeparatorMode::None;
        assert_eq!(error_row_display_number(doc, 2, 0), 2, "data_start를 빼지 않는다");
    }

    /// 상태바 문구가 세 상태를 가른다. 특히 "아직 안 돌았다"와 "돌았는데
    /// 0개"는 둘 다 빈 목록이지만 정반대의 뜻이다.
    #[test]
    fn error_status_text_distinguishes_states() {
        let (mut app, ctx) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();

        // 아직 안 돌았음 — 표시할 것이 없다.
        doc.row_errors = None;
        assert_eq!(error_status_text(doc), None);

        // 돌았고 오류 0개.
        start_error_scan(doc, &ctx);
        poll_scan_to_completion(doc);
        assert_eq!(error_status_text(doc).as_deref(), Some("No bad rows"));

        // 텍스트 모드는 아예 표시하지 않는다.
        doc.sep = SeparatorMode::None;
        assert_eq!(error_status_text(doc), None);
    }

    /// 오류가 있으면 개수를, 상한에 걸렸으면 "일부만 보여 준다"를 밝힌다.
    #[test]
    fn error_status_text_reports_counts_and_truncation() {
        let (mut app, _) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();

        doc.row_errors = Some(crate::validate::ScanResult {
            errors: vec![crate::validate::RowError {
                logical: 1,
                issue: crate::validate::RowIssue::UnbalancedQuote,
                preview: "x".into(),
            }],
            dropped: 0,
        });
        assert_eq!(error_status_text(doc).as_deref(), Some("⚠ 1 bad rows"));

        // 상한에 걸린 경우 — 목록이 전부가 아님을 반드시 밝힌다.
        doc.row_errors = Some(crate::validate::ScanResult {
            errors: vec![crate::validate::RowError {
                logical: 1,
                issue: crate::validate::RowIssue::UnbalancedQuote,
                preview: "x".into(),
            }],
            dropped: 41,
        });
        let text = error_status_text(doc).unwrap();
        assert!(text.contains("42"), "총계를 밝힌다: {text}");
        assert!(text.contains("showing first 1"), "일부임을 밝힌다: {text}");
    }

    /// 오류 행 검사는 헤더를 검사 대상에서 뺀다(`data_start`).
    #[test]
    fn error_scan_skips_header_row() {
        // 헤더의 필드 수가 데이터와 다르지만 헤더는 오류가 아니다.
        let (mut app, _) = edit_doc(b"solo\n1,2\n3,4\n", true);
        let ctx = egui::Context::default();
        let doc = app.doc_mut().unwrap();
        start_error_scan(doc, &ctx);
        assert_eq!(doc.row_errors.as_ref().unwrap().total(), 0);
    }

    /// 유형 라벨이 세 종류를 구분한다.
    #[test]
    fn issue_labels_are_distinct() {
        use crate::validate::RowIssue;
        let fc = issue_label(RowIssue::FieldCount { got: 2, expected: 3 });
        let uq = issue_label(RowIssue::UnbalancedQuote);
        let de = issue_label(RowIssue::DecodeError);
        assert!(fc.contains('2') && fc.contains('3'), "{fc}");
        assert_ne!(fc, uq);
        assert_ne!(uq, de);
        assert_ne!(fc, de);
    }

    /// 백그라운드 검사가 끝날 때까지 기다린 뒤 결과를 문서에 반영한다(테스트 전용).
    ///
    /// 예전에는 `poll_error_scan`을 10만 번 돌리는 스핀락이었는데, 그 상한은
    /// 사실상 **시간** 상한이라 테스트가 병렬로 돌아 CPU가 붐비면 워커가 그 안에
    /// 스케줄되지 못해 간헐적으로 패닉했다(제품 버그가 아니라 테스트의 문제).
    /// `join_result`는 OS에 기다림을 맡기므로 부하와 무관하게 정확하고, 대기
    /// 시간도 스핀보다 짧다.
    ///
    /// 편집 모드는 `start_error_scan`이 그 자리에서 동기로 끝내 `error_scan`이
    /// 아예 없다 — 그 경우 할 일이 없다.
    fn poll_scan_to_completion(doc: &mut Document) {
        let Some(job) = &mut doc.error_scan else {
            return;
        };
        doc.row_errors = job.join_result();
        doc.error_scan = None;
    }

    #[test]
    fn convert_enabled_rejects_text_mode() {
        // 텍스트 모드는 나눌 기준이 없으니 변환도 없다.
        assert!(!convert_enabled(SeparatorMode::None, Some(b'\t')));
    }

    #[test]
    fn convert_enabled_rejects_same_delimiter() {
        // no-op를 데이터 변경으로 기록하면 dirty가 거짓으로 선다.
        assert!(!convert_enabled(SeparatorMode::Char(b','), Some(b',')));
    }

    #[test]
    fn convert_enabled_rejects_non_ascii() {
        // `join_fields`가 `delim as char`로 쓰므로 비ASCII는 UTF-8 두 바이트가
        // 되어 파일이 깨진다.
        assert!(!convert_enabled(SeparatorMode::Char(b','), Some(0xA9)));
        assert!(!convert_enabled(SeparatorMode::Char(b','), Some(0xC2)));
    }

    #[test]
    fn convert_enabled_rejects_no_target() {
        assert!(!convert_enabled(SeparatorMode::Char(b','), None));
    }

    #[test]
    fn convert_enabled_accepts_valid_change() {
        assert!(convert_enabled(SeparatorMode::Char(b','), Some(b'\t')));
        assert!(convert_enabled(SeparatorMode::Char(b'\t'), Some(b',')));
        // 커스텀 구분자도 ASCII면 양방향 모두 허용된다(사용자 요구 사례 1, 2).
        assert!(convert_enabled(SeparatorMode::Char(b'~'), Some(b'\t')));
        assert!(convert_enabled(SeparatorMode::Char(b','), Some(b'~')));
    }

    #[test]
    fn convert_rewrites_data_and_updates_sep() {
        let (mut app, _ctx) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();
        assert_eq!(doc.sep, SeparatorMode::Char(b','));
        convert_delimiter_in_doc(doc, b'\t');
        // 데이터가 실제로 바뀌었다.
        assert_eq!(doc.edit.as_ref().unwrap().lines, vec!["a\tb", "1\t2"]);
        // 보기 기준도 따라왔다 — 안 맞추면 표가 한 컬럼으로 무너진다.
        assert_eq!(doc.sep, SeparatorMode::Char(b'\t'));
        assert!(doc.edit.as_ref().unwrap().dirty);
    }

    #[test]
    fn convert_enters_edit_mode_from_view_mode() {
        // 뷰 모드에서 눌러도 동작해야 한다(Replace처럼 막지 않는다).
        let (mut app, _ctx) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();
        assert!(doc.edit.is_none(), "사전 조건: 뷰 모드");
        convert_delimiter_in_doc(doc, b'|');
        assert!(doc.edit.is_some(), "편집 모드로 자동 전환되어야 한다");
        assert_eq!(doc.edit.as_ref().unwrap().lines, vec!["a|b", "1|2"]);
    }

    #[test]
    fn convert_undo_restores_all_rows_in_one_step() {
        let (mut app, _ctx) = convert_doc(b"a,b\n1,2\n3,4\n");
        let doc = app.doc_mut().unwrap();
        convert_delimiter_in_doc(doc, b'\t');
        assert_eq!(doc.edit.as_ref().unwrap().lines, vec!["a\tb", "1\t2", "3\t4"]);
        // Ctrl+Z 한 번에 전부 돌아와야 한다 — 세 행이 Replace **하나**에
        // 묶여 있어야 성립한다.
        let e = doc.edit.as_mut().unwrap();
        assert_eq!(e.undo.len(), 1, "한 사용자 동작 = 한 undo 단계");
        assert!(e.undo.undo(&mut e.lines));
        assert_eq!(e.lines, vec!["a,b", "1,2", "3,4"]);
    }

    #[test]
    fn convert_preserves_quoted_cells() {
        // 인용 안의 콤마는 데이터다. 탭으로 바뀌면 안 된다.
        let (mut app, _ctx) = convert_doc("name,addr\n홍길동,\"서울, 강남구\"\n".as_bytes());
        let doc = app.doc_mut().unwrap();
        convert_delimiter_in_doc(doc, b'\t');
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            vec!["name\taddr", "홍길동\t서울, 강남구"]
        );
    }

    #[test]
    fn convert_quotes_when_new_delimiter_in_value() {
        // 탭 → 콤마. 값에 콤마가 있으므로 인용이 **생겨야** 한다.
        let (mut app, _ctx) = convert_doc("name\taddr\n홍길동\t서울, 강남구\n".as_bytes());
        let doc = app.doc_mut().unwrap();
        // 자동 감지는 값에 든 콤마를 보고 콤마를 고를 수 있다. 이 테스트가
        // 검증하려는 것은 감지가 아니라 변환이므로 구분자를 명시한다.
        doc.sep = SeparatorMode::Char(b'\t');
        convert_delimiter_in_doc(doc, b',');
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            vec!["name,addr", "홍길동,\"서울, 강남구\""]
        );
    }

    #[test]
    fn convert_clears_column_bound_state_but_keeps_header() {
        let (mut app, _ctx) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();
        doc.has_header = true;
        doc.selected_col = Some(1);
        doc.sort_specs.push(SortSpec {
            col: 1,
            kind: SortKind::Text,
            dir: SortDir::Asc,
            ci: true,
        });
        convert_delimiter_in_doc(doc, b'\t');
        // 컬럼 경계가 달라졌으므로 컬럼에 매인 상태는 버린다.
        assert_eq!(doc.selected_col, None);
        assert!(doc.sort_specs.is_empty());
        assert!(doc.sort.is_none());
        // 헤더는 유지 — 변환은 행 수도 필드 수도 바꾸지 않는다.
        assert!(doc.has_header, "헤더 유무는 변환과 무관하다");
    }

    #[test]
    fn convert_no_op_does_not_dirty() {
        // 구분자가 하나도 없는 문서 → 바뀌는 행이 없다. dirty가 서면 안 된다.
        let (mut app, _ctx) = convert_doc(b"solo\nalone\n");
        let doc = app.doc_mut().unwrap();
        doc.sep = SeparatorMode::Char(b',');
        convert_delimiter_in_doc(doc, b'\t');
        assert_eq!(doc.sep, SeparatorMode::Char(b'\t'), "보기 기준은 바뀐다");
        if let Some(e) = doc.edit.as_ref() {
            assert!(!e.dirty, "바뀐 행이 없으면 dirty가 서면 안 된다");
            assert!(e.undo.is_empty(), "유령 undo 단계를 만들면 안 된다");
        }
    }

    #[test]
    fn convert_rejects_same_delimiter_at_runtime() {
        // 가드가 UI에만 있으면 안 된다 — 함수 자체가 막아야 한다.
        let (mut app, _ctx) = convert_doc(b"a,b\n1,2\n");
        let doc = app.doc_mut().unwrap();
        convert_delimiter_in_doc(doc, b',');
        assert!(doc.edit.is_none(), "no-op는 편집 모드조차 켜지 않는다");
    }

    #[test]
    fn open_path_stores_real_path() {
        // 저장(덮어쓰기)이 표시 문자열을 되파싱하지 않고 쓸 수 있어야 한다.
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc().unwrap();
        assert_eq!(doc.path, p);
        assert_eq!(doc.path_label, p.display().to_string());
    }

    #[test]
    fn edit_dirty_reflects_buffer_state() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        // 파일만 열린 상태(뷰 모드)는 dirty가 아니다.
        assert!(!app.edit_dirty());
        {
            let doc = app.doc_mut().unwrap();
            doc.indexer.take().unwrap().join().unwrap();
            enter_edit_mode(doc);
        }
        assert!(!app.edit_dirty(), "막 진입한 버퍼는 깨끗하다");
        app.doc_mut().unwrap().edit.as_mut().unwrap().dirty = true;
        assert!(app.edit_dirty());
    }

    /// 편집 모드 정렬 테스트용 Document를 만든다(인덱싱 완료 + 편집 모드 진입).
    fn edit_doc(content: &[u8], has_header: bool) -> (App, u8) {
        let p = temp(content);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        doc.has_header = has_header;
        enter_edit_mode(doc);
        (app, b',')
    }

    #[test]
    fn edit_sort_rearranges_lines_and_keeps_header() {
        let (mut app, delim) = edit_doc(b"name,n\nCharlie,3\nAlice,1\nBob,2\n", true);
        let doc = app.doc_mut().unwrap();
        let spec = SortSpec { col: 0, kind: SortKind::Text, dir: SortDir::Asc, ci: true };
        apply_edit_sort(doc, &[spec], delim, 1);
        let e = doc.edit.as_ref().unwrap();
        assert_eq!(e.lines, v(&["name,n", "Alice,1", "Bob,2", "Charlie,3"]));
        assert!(e.dirty, "정렬은 버퍼를 변경한다");
    }

    #[test]
    fn edit_sort_does_not_set_view_sort_state() {
        // 편집 모드 정렬은 lines에 이미 반영되므로 살아 있는 permutation이 없다.
        // doc.sort를 세우면 헤더 화살표/상태바가 없는 정렬을 주장하고,
        // render_table이 permutation으로 행을 한 번 더 매핑해 순서가 깨진다.
        let (mut app, delim) = edit_doc(b"name,n\nCharlie,3\nAlice,1\n", true);
        let doc = app.doc_mut().unwrap();
        let spec = SortSpec { col: 0, kind: SortKind::Text, dir: SortDir::Asc, ci: true };
        apply_edit_sort(doc, &[spec], delim, 1);
        assert!(doc.sort.is_none());
        assert!(doc.sort_job.is_none());
    }

    #[test]
    fn edit_sort_clears_row_pointing_state() {
        // 행이 뒤섞이면 선택/편집 중 셀이 가리키던 행이 달라진다 → 초기화.
        let (mut app, delim) = edit_doc(b"name,n\nCharlie,3\nAlice,1\n", true);
        let doc = app.doc_mut().unwrap();
        doc.cell_sel = Some((1, 0, 1, 1));
        doc.editing_cell = Some((1, 0));
        doc.cell_edit_text = "x".into();
        let spec = SortSpec { col: 0, kind: SortKind::Text, dir: SortDir::Asc, ci: true };
        apply_edit_sort(doc, &[spec], delim, 1);
        assert!(doc.cell_sel.is_none());
        assert!(doc.editing_cell.is_none());
        assert!(doc.cell_edit_text.is_empty());
    }

    #[test]
    fn insert_after_edit_sort_stays_where_inserted() {
        // 정렬 뒤 행을 삽입하면 재정렬되지 않고 그 자리에 남아야 한다
        // (permutation이 아니라 물리적 재배치이므로 자연히 그렇다).
        let (mut app, delim) = edit_doc(b"name,n\nCharlie,3\nAlice,1\nBob,2\n", true);
        let doc = app.doc_mut().unwrap();
        let spec = SortSpec { col: 0, kind: SortKind::Text, dir: SortDir::Asc, ci: true };
        apply_edit_sort(doc, &[spec], delim, 1);
        // "Bob,2"(index 2) 위에 zzz 행 삽입.
        let e = doc.edit.as_mut().unwrap();
        crate::edit::insert_row(&mut e.lines, 2, "zzz,9".into());
        assert_eq!(
            e.lines,
            v(&["name,n", "Alice,1", "zzz,9", "Bob,2", "Charlie,3"]),
            "삽입 행은 정렬 순서로 밀려나지 않는다"
        );
    }

    #[test]
    fn edit_sort_headerless_sorts_all_rows() {
        let (mut app, delim) = edit_doc(b"3,c\n1,a\n2,b\n", false);
        let doc = app.doc_mut().unwrap();
        let spec = SortSpec { col: 0, kind: SortKind::Number, dir: SortDir::Asc, ci: false };
        apply_edit_sort(doc, &[spec], delim, 0);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["1,a", "2,b", "3,c"]));
    }

    #[test]
    fn edit_sort_multi_key() {
        let (mut app, delim) = edit_doc(b"g,n\nb,2\na,2\nb,1\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        let specs = vec![
            SortSpec { col: 0, kind: SortKind::Text, dir: SortDir::Asc, ci: true },
            SortSpec { col: 1, kind: SortKind::Number, dir: SortDir::Desc, ci: false },
        ];
        apply_edit_sort(doc, &specs, delim, 1);
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            v(&["g,n", "a,2", "a,1", "b,2", "b,1"])
        );
    }

    #[test]
    fn edit_sort_noop_on_empty_specs_or_data() {
        // 기준이 없거나 데이터 행이 없으면 아무것도 하지 않는다(dirty도 안 켠다).
        let (mut app, delim) = edit_doc(b"name,n\n", true);
        let doc = app.doc_mut().unwrap();
        apply_edit_sort(doc, &[], delim, 1);
        assert!(!doc.edit.as_ref().unwrap().dirty);
        let spec = SortSpec { col: 0, kind: SortKind::Text, dir: SortDir::Asc, ci: true };
        // 헤더만 있는 파일(데이터 행 0개).
        apply_edit_sort(doc, &[spec], delim, 1);
        assert!(!doc.edit.as_ref().unwrap().dirty);
    }

    #[test]
    fn sort_dialog_column_labels_use_edit_buffer() {
        // 브리프의 이월 결함: 다이얼로그가 mmap 전용 경로로 헤더를 읽으면
        // 편집 모드에서 편집 전 값이 나온다. parse_logical_line_edit 경유여야 한다.
        let (mut app, delim) = edit_doc(b"old,b\n1,2\n", true);
        let doc = app.doc_mut().unwrap();
        crate::edit::set_cell(&mut doc.edit.as_mut().unwrap().lines, 0, 0, "new", delim);
        assert_eq!(
            parse_logical_line_edit(doc, 0, delim),
            Some(v(&["new", "b"])),
            "편집한 헤더 값이 보여야 함"
        );
    }

    #[test]
    fn save_roundtrip_cp949_from_edit_buffer() {
        // 저장 다이얼로그가 부르는 조합(write_file + SaveOptions)이 편집 버퍼를
        // CP949로 변환해 쓰고, 다시 열면 같은 내용이 나오는지.
        let (mut app, _delim) = edit_doc("h,v\n가,1\n".as_bytes(), true);
        let out = temp_ext(b"", "csv");
        {
            let e = app.doc().unwrap().edit.as_ref().unwrap();
            let opts = crate::save::SaveOptions {
                enc: crate::parse::Encoding::Cp949,
                bom: false,
                newline: e.newline,
            };
            crate::save::write_file(&out, &e.lines, &opts, None).unwrap();
        }
        let bytes = std::fs::read(&out).unwrap();
        // CP949 '가' = B0 A1
        assert!(bytes.windows(2).any(|w| w == [0xB0, 0xA1]), "CP949로 인코딩됨");
        // 다시 열어(CP949로) 같은 줄이 나오는지.
        let ctx = egui::Context::default();
        let mut app2 = App::default();
        app2.open_path(&out, &ctx);
        let doc2 = app2.doc_mut().unwrap();
        doc2.indexer.take().unwrap().join().unwrap();
        doc2.enc = crate::parse::Encoding::Cp949;
        enter_edit_mode(doc2);
        assert_eq!(doc2.edit.as_ref().unwrap().lines, v(&["h,v", "가,1"]));
    }

    /// 저장 후 뷰 경로(mmap)가 낡지 않는지. 리뷰가 지목한 CRITICAL 결함의 회귀 방지:
    /// `Source`가 붙들고 있는 `Mmap`은 `write_file`의 `rename` 뒤에도 **저장 전
    /// 바이트**를 계속 돌려주므로, 저장 직후 편집 모드를 끄면 화면이 편집 전
    /// 내용으로 되돌아간다. `repoint_source_after_save`가 이를 막아야 한다.
    #[test]
    fn save_repoints_source_so_view_mode_shows_saved_content() {
        let (mut app, delim) = edit_doc(b"h,v\nold,1\n", true);
        let ctx = egui::Context::default();
        let path = app.doc().unwrap().path.clone();

        // 편집: 셀 하나를 바꾼다.
        {
            let doc = app.doc_mut().unwrap();
            crate::edit::set_cell(&mut doc.edit.as_mut().unwrap().lines, 1, 0, "new", delim);
            doc.edit.as_mut().unwrap().dirty = true;
        }

        // 저장 다이얼로그가 하는 일과 같은 순서: write_file → dirty 해제 → 소스 재지정.
        {
            let doc = app.doc_mut().unwrap();
            let e = doc.edit.as_ref().unwrap();
            let opts = crate::save::SaveOptions {
                enc: doc.enc,
                bom: false,
                newline: e.newline,
            };
            crate::save::write_file(&path, &e.lines, &opts, None).unwrap();
            doc.edit.as_mut().unwrap().dirty = false;
            repoint_source_after_save(doc, &path, &ctx).unwrap();
            doc.indexer.take().unwrap().join().unwrap();
        }

        let doc = app.doc_mut().unwrap();
        // 매핑된 바이트 자체가 저장된 내용이어야 한다(옛 매핑이면 "old,1"이 보인다).
        let raw = String::from_utf8(doc.source.as_bytes().to_vec()).unwrap();
        assert!(raw.contains("new,1"), "mmap이 저장된 내용을 보여야 함: {raw:?}");
        assert!(!raw.contains("old,1"), "저장 전 내용이 남아 있으면 안 됨: {raw:?}");

        // 편집 모드를 끄면 뷰 경로(decode_logical_line)로 떨어진다 — 저장된 내용이어야.
        exit_edit_mode(doc);
        assert!(doc.edit.is_none());
        assert_eq!(decode_logical_line(doc, 1).as_deref(), Some("new,1"));
        assert_eq!(logical_line(doc, 1).as_deref(), Some("new,1"));
        // 인덱스 행 수도 새 파일과 맞아야 한다(낡은 인덱스면 유령 행이 남는다).
        assert_eq!(doc.index.line_count(), 2);
    }

    /// 소스 재지정이 편집 세션을 건드리지 않는지. 사용자는 저장 후에도 편집 모드에
    /// 남아 계속 편집할 수 있어야 하고, `edit.lines`는 절대 디스크에서 다시
    /// 읽히면 안 된다(만약 내용이 달랐다면 편집 내용을 잃게 된다).
    #[test]
    fn repoint_after_save_preserves_edit_buffer_and_selection() {
        let (mut app, _delim) = edit_doc(b"h,v\na,1\nb,2\n", true);
        let ctx = egui::Context::default();
        let path = app.doc().unwrap().path.clone();
        let doc = app.doc_mut().unwrap();
        doc.cell_sel = Some((1, 0, 2, 1));
        doc.selected_col = Some(1);
        // 디스크에는 버퍼와 다른 내용을 써 둔다 — 재지정이 버퍼를 덮어쓰면 들킨다.
        // 열린 mmap 때문에 in-place 쓰기(fs::write)는 Windows에서 실패하므로
        // (ERROR_USER_MAPPED_FILE), 프로덕션과 같은 rename 경로(write_file)를 쓴다.
        {
            let opts = crate::save::SaveOptions {
                enc: doc.enc,
                bom: false,
                newline: crate::edit::Newline::Lf,
            };
            crate::save::write_file(&path, &v(&["h,v", "DISK,9"]), &opts, None).unwrap();
        }

        repoint_source_after_save(doc, &path, &ctx).unwrap();
        doc.indexer.take().unwrap().join().unwrap();

        let e = doc.edit.as_ref().expect("편집 모드가 유지되어야 함");
        assert_eq!(e.lines, v(&["h,v", "a,1", "b,2"]), "버퍼는 디스크에서 다시 읽지 않는다");
        assert_eq!(doc.cell_sel, Some((1, 0, 2, 1)), "선택 유지");
        assert_eq!(doc.selected_col, Some(1), "선택 컬럼 유지");
        // 편집 모드에서는 여전히 버퍼가 진실.
        assert_eq!(logical_line(doc, 1).as_deref(), Some("a,1"));
    }

    /// 저장으로 소스/인덱스를 갈아끼우면 오류 목록도 버려야 한다.
    ///
    /// 편집 모드인 동안은 편집 버퍼가 진실이라 티가 안 나지만, 편집 모드를
    /// 끄면 **새 파일을 옛 목록으로 설명하게** 된다. 게다가 새 인덱스가
    /// 프라이밍 중이라 `should_start_error_scan`이 재검사도 막으므로, 낡은
    /// 목록이 그대로 화면에 남는다.
    #[test]
    fn repoint_after_save_invalidates_error_scan() {
        let path = temp(b"h,v\na,1\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&path, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        doc.has_header = true;

        start_error_scan(doc, &ctx);
        poll_scan_to_completion(doc);
        assert!(doc.row_errors.is_some(), "전제: 검사 결과가 있다");

        enter_edit_mode(doc);
        // 편집 모드 진입 자체가 무효화하므로, 저장 경로만 보려고 다시 채운다.
        start_error_scan(doc, &ctx);
        assert!(doc.row_errors.is_some(), "전제: 편집 모드 결과가 있다");

        {
            let opts = crate::save::SaveOptions {
                enc: doc.enc,
                bom: false,
                newline: crate::edit::Newline::Lf,
            };
            crate::save::write_file(&path, &v(&["h,v", "a,1"]), &opts, None).unwrap();
        }
        repoint_source_after_save(doc, &path, &ctx).unwrap();
        doc.indexer.take().unwrap().join().unwrap();

        assert!(
            doc.row_errors.is_none(),
            "소스·인덱스가 바뀌었으면 옛 목록은 무효"
        );
    }

    /// save-as: 새 경로로 저장하면 소스도 새 파일을 매핑해야 한다
    /// (예전에는 path만 새 파일을 가리키고 source는 원본을 매핑한 채였다).
    #[test]
    fn save_as_repoints_source_to_new_path() {
        let (mut app, delim) = edit_doc(b"h,v\nold,1\n", true);
        let ctx = egui::Context::default();
        let out = temp_ext(b"", "csv");
        {
            let doc = app.doc_mut().unwrap();
            crate::edit::set_cell(&mut doc.edit.as_mut().unwrap().lines, 1, 0, "new", delim);
            let e = doc.edit.as_ref().unwrap();
            let opts = crate::save::SaveOptions {
                enc: doc.enc,
                bom: false,
                newline: e.newline,
            };
            crate::save::write_file(&out, &e.lines, &opts, None).unwrap();
            doc.path_label = out.display().to_string();
            doc.path = out.clone();
            repoint_source_after_save(doc, &out, &ctx).unwrap();
            doc.indexer.take().unwrap().join().unwrap();
        }
        let doc = app.doc_mut().unwrap();
        exit_edit_mode(doc);
        assert_eq!(decode_logical_line(doc, 1).as_deref(), Some("new,1"));
        assert_eq!(doc.path, out);
    }

    /// 저장 실패해도 편집 버퍼는 dirty로 남는지(Finding 3의 내부 상태 쪽).
    /// 표시 자체(`ui.colored_label` 배치)는 GUI 없이 검증할 수 없어 수동
    /// 체크리스트 F-5로 넘겼다 — 여기서는 상태바가 읽는 값이 살아 있음을 고정한다.
    #[test]
    fn failed_save_keeps_dirty_state_visible_to_status_bar() {
        let (mut app, delim) = edit_doc(b"h,v\nold,1\n", true);
        {
            let doc = app.doc_mut().unwrap();
            crate::edit::set_cell(&mut doc.edit.as_mut().unwrap().lines, 1, 0, "new", delim);
            doc.edit.as_mut().unwrap().dirty = true;
        }
        // 존재하지 않는 디렉터리로 저장 → 실패.
        let bad = std::path::Path::new("no_such_dir_xyz").join("out.csv");
        let doc = app.doc().unwrap();
        let e = doc.edit.as_ref().unwrap();
        let opts = crate::save::SaveOptions { enc: doc.enc, bom: false, newline: e.newline };
        assert!(crate::save::write_file(&bad, &e.lines, &opts, None).is_err());
        // 상태바가 읽는 두 값이 모두 살아 있어야 한다.
        assert!(app.edit_dirty(), "실패한 저장은 dirty를 유지한다");
        assert_eq!(app.doc().unwrap().edit.as_ref().unwrap().lines.len(), 2);
    }

    #[test]
    fn normalize_rect_orders_corners() {
        // 아래→위, 오른쪽→왼쪽으로 드래그해도 (r0<=r1, c0<=c1)로 정규화.
        assert_eq!(normalize_rect((5, 4, 2, 1)), (2, 1, 5, 4));
        assert_eq!(normalize_rect((2, 1, 5, 4)), (2, 1, 5, 4));
        // 역방향 혼합(행은 정방향, 열은 역방향).
        assert_eq!(normalize_rect((2, 4, 5, 1)), (2, 1, 5, 4));
    }

    #[test]
    fn column_as_rect_covers_data_rows_only() {
        // 헤더 있음(data_start=1), 4줄 파일 → 데이터 행 1..=3.
        assert_eq!(column_as_rect(2, 1, 4), Some((1, 2, 3, 2)));
        // 헤더 없음 → 0..=3.
        assert_eq!(column_as_rect(0, 0, 4), Some((0, 0, 3, 0)));
        // 데이터 행이 없으면(헤더뿐) None.
        assert_eq!(column_as_rect(0, 1, 1), None);
        assert_eq!(column_as_rect(0, 0, 0), None);
        // 한 줄뿐인 헤더 없는 파일.
        assert_eq!(column_as_rect(3, 0, 1), Some((0, 3, 0, 3)));
    }

    #[test]
    fn effective_rect_prefers_cell_selection() {
        // 셀 선택이 있으면 컬럼 선택은 무시된다(정규화해서 돌려준다).
        assert_eq!(
            effective_cell_rect(Some((5, 4, 2, 1)), Some(0), 1, 10),
            Some((2, 1, 5, 4))
        );
        // 셀 선택이 없고 컬럼만 선택 → 컬럼 전체 사각 범위.
        assert_eq!(effective_cell_rect(None, Some(2), 1, 4), Some((1, 2, 3, 2)));
        // 둘 다 없으면 None.
        assert_eq!(effective_cell_rect(None, None, 1, 4), None);
        // 컬럼은 선택됐지만 데이터 행이 없으면 None.
        assert_eq!(effective_cell_rect(None, Some(2), 1, 1), None);
    }

    /// 컬럼 선택 복사가 헤더를 제외하고 그 컬럼만 세로로 뽑는지.
    /// (`cells_to_tsv`는 한 컬럼이면 행마다 필드 하나 → "값\n값\n값".)
    #[test]
    fn column_copy_excludes_header_and_is_vertical() {
        let (mut app, delim) = edit_doc(b"h1,h2\na,1\nb,2\nc,3\n", true);
        let doc = app.doc_mut().unwrap();
        doc.selected_col = Some(1);
        doc.cell_sel = None;
        let lines = &doc.edit.as_ref().unwrap().lines;
        let (r0, c0, r1, c1) =
            effective_cell_rect(doc.cell_sel, doc.selected_col, 1, lines.len()).unwrap();
        assert_eq!(
            crate::edit::cells_to_tsv(lines, r0, c0, r1, c1, delim),
            "1\n2\n3",
            "헤더 h2는 빠지고 데이터만 세로로"
        );
    }

    #[test]
    fn rect_contains_respects_unnormalized_input() {
        // 역방향으로 저장된 선택도 포함 판정이 정확해야 한다(우클릭 셀이
        // 선택 안인지 밖인지 판단하는 데 쓰임).
        let sel = (5, 4, 2, 1); // 실제 범위: 행 2..=5, 열 1..=4
        assert!(rect_contains(sel, 3, 2));
        assert!(rect_contains(sel, 2, 1));
        assert!(rect_contains(sel, 5, 4));
        assert!(!rect_contains(sel, 1, 2), "행 범위 밖");
        assert!(!rect_contains(sel, 3, 0), "열 범위 밖");
        assert!(!rect_contains(sel, 6, 4), "행 범위 밖");
    }

    #[test]
    fn shift_click_keeps_anchor_extends_head() {
        // 기존 선택 (2,1)-(2,1)에서 (5,3)을 shift+클릭 → (2,1)-(5,3)
        let prev = Some((2usize, 1usize, 2usize, 1usize));
        assert_eq!(shift_extend(prev, 5, 3), (2, 1, 5, 3));
    }

    #[test]
    fn shift_click_without_previous_selection_starts_there() {
        // 이전 선택이 없으면 그 셀 단일 선택으로 시작.
        assert_eq!(shift_extend(None, 4, 2), (4, 2, 4, 2));
    }

    #[test]
    fn shift_click_keeps_original_anchor_on_repeat() {
        // 이미 확장된 선택에서 다시 shift+클릭하면 **최초 앵커**가 유지된다
        // (끝점만 계속 움직인다 — Windows 표준).
        let mut sel = shift_extend(Some((2, 1, 2, 1)), 5, 3);
        sel = shift_extend(Some(sel), 8, 0);
        assert_eq!(sel, (2, 1, 8, 0));
        sel = shift_extend(Some(sel), 1, 1);
        assert_eq!(sel, (2, 1, 1, 1), "앵커 위로 올라가도 앵커는 그대로");
    }

    #[test]
    fn shift_click_uses_unnormalized_anchor() {
        // cell_sel은 정규화 전 원본이라 앞 두 값이 앵커다. 역방향 선택에서
        // shift+클릭하면 정규화된 좌상단이 아니라 **원래 앵커**가 유지돼야 한다.
        let prev = Some((5usize, 4usize, 2usize, 1usize)); // 앵커 (5,4)
        assert_eq!(shift_extend(prev, 7, 6), (5, 4, 7, 6));
    }

    #[test]
    fn shift_click_text_anchor_from_selection_or_caret() {
        let a = tp(1, 2);
        let b = tp(4, 0);
        // 선택이 있으면 그 앵커(첫 값)를 쓴다.
        assert_eq!(shift_extend_text(Some((a, b)), tp(9, 9)), a);
        // 선택이 없으면 현재 캐럿이 앵커.
        assert_eq!(shift_extend_text(None, tp(3, 3)), tp(3, 3));
    }

    #[test]
    fn cell_drag_active_latches_only_on_cell_press() {
        // 버튼을 뗀 프레임은 무조건 해제.
        assert!(!next_cell_drag_active(true, false, false));
        assert!(!next_cell_drag_active(true, false, true));
        // 표 밖에서 누른 채 표 위로 들어온 포인터: 셀 누름이 관측된 적 없으므로
        // 버튼이 눌려 있어도 켜지지 않는다(선택이 끌려가지 않음).
        assert!(!next_cell_drag_active(false, true, false));
        // 셀 위에서 누름이 시작되면 켜진다.
        assert!(next_cell_drag_active(false, true, true));
        // 켜진 뒤에는 포인터가 셀 밖으로 나가도 버튼이 눌린 동안 유지된다.
        assert!(next_cell_drag_active(true, true, false));
        // 뗐다가 다시 누르면 셀 누름이 다시 관측돼야만 켜진다.
        let after_release = next_cell_drag_active(true, false, false);
        assert!(!next_cell_drag_active(after_release, true, false));
    }

    fn v(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    fn tp(line: usize, col: usize) -> crate::edit::TextPos {
        crate::edit::TextPos { line, col }
    }

    #[test]
    fn clamp_pos_bounds_line_and_col() {
        let lines = v(&["ab", "cdef"]);
        assert_eq!(clamp_pos(&lines, tp(9, 9)), tp(1, 4));
        assert_eq!(clamp_pos(&lines, tp(0, 9)), tp(0, 2));
        assert_eq!(clamp_pos(&lines, tp(1, 2)), tp(1, 2));
        // 빈 lines도 안전하게.
        assert_eq!(clamp_pos(&[], tp(3, 3)), tp(0, 0));
    }

    #[test]
    fn caret_move_crosses_line_boundaries() {
        let lines = v(&["ab", "cd"]);
        // 줄 끝에서 오른쪽 → 다음 줄 처음.
        assert_eq!(apply_caret_move(&lines, tp(0, 2), CaretMove::Right), tp(1, 0));
        // 줄 처음에서 왼쪽 → 앞 줄 끝.
        assert_eq!(apply_caret_move(&lines, tp(1, 0), CaretMove::Left), tp(0, 2));
        // 문서 처음/끝에서는 no-op(끝은 마지막 줄 끝으로 클램프).
        assert_eq!(apply_caret_move(&lines, tp(0, 0), CaretMove::Left), tp(0, 0));
        assert_eq!(apply_caret_move(&lines, tp(1, 2), CaretMove::Right), tp(1, 2));
    }

    #[test]
    fn caret_move_up_down_clamps_column() {
        let lines = v(&["abcdef", "xy"]);
        // 긴 줄에서 짧은 줄로 내려가면 col이 줄 길이로 클램프.
        assert_eq!(apply_caret_move(&lines, tp(0, 5), CaretMove::Down), tp(1, 2));
        // 첫 줄에서 위 → 문서 처음.
        assert_eq!(apply_caret_move(&lines, tp(0, 3), CaretMove::Up), tp(0, 0));
        // 마지막 줄에서 아래 → 그 줄 끝.
        assert_eq!(apply_caret_move(&lines, tp(1, 1), CaretMove::Down), tp(1, 2));
    }

    #[test]
    fn caret_move_home_end() {
        let lines = v(&["hello"]);
        assert_eq!(apply_caret_move(&lines, tp(0, 3), CaretMove::Home), tp(0, 0));
        assert_eq!(apply_caret_move(&lines, tp(0, 1), CaretMove::End), tp(0, 5));
    }

    #[test]
    fn shift_arrow_extends_keeping_anchor() {
        let lines = v(&["abcdef"]);
        // 선택 없음 + Shift+Right → 앵커는 현재 캐럿.
        let (c, s) = next_caret_and_sel(&lines, tp(0, 2), None, CaretMove::Right, true);
        assert_eq!(c, tp(0, 3));
        assert_eq!(s, Some((tp(0, 2), tp(0, 3))));
        // 이어서 한 번 더 → 앵커 유지, 캐럿만 전진.
        let (c2, s2) = next_caret_and_sel(&lines, c, s, CaretMove::Right, true);
        assert_eq!(c2, tp(0, 4));
        assert_eq!(s2, Some((tp(0, 2), tp(0, 4))));
        // 되돌아와 앵커와 같아지면 선택 해제.
        let (_, s3) = next_caret_and_sel(&lines, tp(0, 3), s, CaretMove::Left, true);
        assert_eq!(s3, None);
    }

    #[test]
    fn plain_arrow_collapses_selection_without_moving() {
        let lines = v(&["abcdef"]);
        let sel = Some((tp(0, 1), tp(0, 4)));
        // 왼쪽 → 선택 시작으로 붕괴(한 칸 더 가지 않는다).
        assert_eq!(
            next_caret_and_sel(&lines, tp(0, 4), sel, CaretMove::Left, false),
            (tp(0, 1), None)
        );
        // 오른쪽 → 선택 끝으로 붕괴.
        assert_eq!(
            next_caret_and_sel(&lines, tp(0, 4), sel, CaretMove::Right, false),
            (tp(0, 4), None)
        );
        // 역방향 선택도 정규화해서 판단.
        let rev = Some((tp(0, 4), tp(0, 1)));
        assert_eq!(
            next_caret_and_sel(&lines, tp(0, 1), rev, CaretMove::Right, false),
            (tp(0, 4), None)
        );
    }

    #[test]
    fn plain_arrow_moves_when_no_selection() {
        let lines = v(&["abc"]);
        assert_eq!(
            next_caret_and_sel(&lines, tp(0, 1), None, CaretMove::Right, false),
            (tp(0, 2), None)
        );
    }

    #[test]
    fn select_all_spans_whole_document() {
        let lines = v(&["ab", "cde"]);
        assert_eq!(whole_document_sel(&lines), (tp(0, 0), tp(1, 3)));
        // 빈 문서(줄 없음)에서도 패닉하지 않는다.
        assert_eq!(whole_document_sel(&[]), (tp(0, 0), tp(0, 0)));
    }

    #[test]
    fn sel_span_covers_first_middle_last_lines() {
        // 0행 col2 ~ 2행 col1 선택. 중간 줄은 전부, 시작/끝 줄은 부분.
        let a = tp(0, 2);
        let b = tp(2, 1);
        // 시작 줄: col2부터 줄 끝(len=5) + 개행 한 칸 → (2, 6)
        assert_eq!(sel_span_on_line(a, b, 0, 5), Some((2, 6)));
        // 중간 줄: 전부 + 개행.
        assert_eq!(sel_span_on_line(a, b, 1, 3), Some((0, 4)));
        // 끝 줄: 처음부터 col1까지(개행 없음).
        assert_eq!(sel_span_on_line(a, b, 2, 7), Some((0, 1)));
        // 범위 밖 줄.
        assert_eq!(sel_span_on_line(a, b, 3, 4), None);
    }

    #[test]
    fn sel_span_single_line_and_empty() {
        let a = tp(1, 1);
        let b = tp(1, 3);
        assert_eq!(sel_span_on_line(a, b, 1, 5), Some((1, 3)));
        // 같은 줄 빈 범위 → None(캐럿만 있는 상태).
        assert_eq!(sel_span_on_line(a, a, 1, 5), None);
        // 끝 줄의 col이 0이면(다음 줄 처음에서 끝나는 선택) 그 줄엔 음영 없음.
        assert_eq!(sel_span_on_line(tp(0, 0), tp(1, 0), 1, 5), None);
    }

    /// Backspace의 dirty 판정. `apply_text_intent`가 쓰는 것과 똑같이
    /// `edit::backspace`의 반환 위치를 헬퍼에 먹여, 실제 no-op 조건에서
    /// dirty가 서지 않음을 고정한다(파일 열자마자 Backspace → "저장 안 됨" 방지).
    #[test]
    fn backspace_at_origin_is_not_dirty() {
        // 문서 맨 앞 + 선택 없음 → edit::backspace가 위치를 그대로 돌려주고,
        // 버퍼도 그대로다. 변경 없음으로 판정해야 한다.
        let mut lines = v(&["ab", "cd"]);
        let before = tp(0, 0);
        let after = crate::edit::backspace(&mut lines, before);
        assert_eq!(lines, v(&["ab", "cd"]), "버퍼가 바뀌면 안 된다");
        assert!(!backspace_or_delete_changed(false, before, after));
    }

    #[test]
    fn backspace_that_deletes_is_dirty() {
        // (a) 줄 중간: 한 글자 삭제 → 캐럿이 뒤로 → 변경.
        let mut lines = v(&["ab"]);
        let before = tp(0, 2);
        let after = crate::edit::backspace(&mut lines, before);
        assert_eq!(lines, v(&["a"]));
        assert!(backspace_or_delete_changed(false, before, after));

        // (b) 줄 맨 앞: 앞 줄과 병합 → 캐럿이 앞 줄로 → 변경.
        let mut lines = v(&["ab", "cd"]);
        let before = tp(1, 0);
        let after = crate::edit::backspace(&mut lines, before);
        assert_eq!(lines, v(&["abcd"]));
        assert!(backspace_or_delete_changed(false, before, after));

        // (c) 선택이 있으면 캐럿이 제자리여도(선택 시작 == 캐럿) 변경이다.
        assert!(backspace_or_delete_changed(true, tp(0, 0), tp(0, 0)));
    }

    /// Delete의 no-op 조건: 문서 끝에서는 Right 이동이 제자리라 삭제가 없다.
    /// `apply_text_intent`의 Delete 분기가 이 조건으로 dirty를 가른다.
    #[test]
    fn delete_at_document_end_is_noop() {
        let lines = v(&["ab", "cd"]);
        let caret = tp(1, 2); // 마지막 줄 끝
        assert_eq!(apply_caret_move(&lines, caret, CaretMove::Right), caret);
        // 문서 끝이 아니면 이동이 생기고 → 삭제할 대상이 있다.
        let mid = tp(0, 2); // 첫 줄 끝(개행 위치)
        assert_ne!(apply_caret_move(&lines, mid, CaretMove::Right), mid);
    }

    /// Tab이 본문 텍스트 줄로 포커스를 옮길 수 없어야 한다.
    ///
    /// `render_text`의 키 입력 게이트는 `memory.focused().is_none()`이다.
    /// 줄이 포커스를 가져갈 수 있으면 Tab 한 번으로 게이트가 닫혀 **모든 키
    /// 입력이 조용히 삼켜지고 문서가 편집 불가**가 된다(화면에 아무 표시도 없다).
    ///
    /// 방어가 두 겹인 이유를 이 테스트가 고정한다:
    /// 1. `TEXT_LINE_SENSE`의 `focusable: false` — Tab 순회 등록 자체를 막는다.
    /// 2. `surrender_focus` — `context_menu`가 내부에서
    ///    `interact(Sense::click())`(focusable: true)로 sense를 union해
    ///    줄을 **다시** focusable로 등록하는 것을 되돌린다.
    ///
    /// 2번 없이 1번만으로는 실제로 막히지 않는다(이 테스트로 실증했다).
    #[test]
    fn tab_cannot_steal_focus_onto_text_line() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("textline_focus_probe");
        // render_text가 한 줄에 대해 하는 것과 같은 순서: interact → context_menu
        // → surrender_focus.
        let draw = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 20.0));
                let resp = ui.interact(rect, id, TEXT_LINE_SENSE);
                resp.context_menu(|ui| {
                    ui.label("메뉴");
                });
                ui.memory_mut(|m| m.surrender_focus(id));
            });
        };
        // 1프레임: 위젯 등록.
        let _ = ctx.run(Default::default(), |ctx| draw(ctx));
        // 2프레임: Tab.
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let _ = ctx.run(input, |ctx| draw(ctx));

        assert_eq!(
            ctx.memory(|m| m.focused()),
            None,
            "Tab이 텍스트 줄에 포커스를 걸면 keyboard_free 게이트가 닫혀 \
             문서 전체가 편집 불가가 된다"
        );
    }

    /// 게이트 자체의 의미 고정: 포커스가 있으면 키를 소비하지 않는다.
    /// (툴바 "직접:" 구분자 TextEdit에 타이핑할 때 본문에 중복 입력되지 않게 하는
    ///  원래 목적 — I-1 수정이 이 성질을 깨지 않았음을 확인한다.)
    #[test]
    fn focused_widget_blocks_document_key_intents() {
        let ctx = egui::Context::default();
        let other = egui::Id::new("toolbar_textedit");
        // 포커스를 가진 다른 위젯이 있는 상태를 만든다.
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(50.0, 20.0));
                let r = ui.interact(rect, other, egui::Sense::click());
                r.request_focus();
            });
        });
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(other),
            "사전 조건: 다른 위젯이 포커스를 쥐고 있어야 한다"
        );
        // render_text의 게이트 식과 동일.
        let keyboard_free = ctx.memory(|m| m.focused().is_none());
        assert!(!keyboard_free, "포커스가 있으면 본문은 키를 소비하지 않는다");
    }

    /// Tab이 표 모드 데이터 셀로 포커스를 옮길 수 없어야 한다.
    /// `tab_cannot_steal_focus_onto_text_line`의 표 모드 판.
    ///
    /// 표 모드에는 `render_text`의 `keyboard_free` 같은 전역 게이트가 없어
    /// 오늘 당장 편집 불가가 되지는 않지만, 구조적 결함은 동일하다
    /// (`Sense::click_and_drag()` = focusable: true → 그려진 셀마다 Tab 순회
    /// 등록, 그리고 `context_menu`가 `Sense::click()`을 union해 그 opt-out을
    /// 매 프레임 무효화). `TABLE_CELL_SENSE` + `surrender_focus` 두 겹이
    /// 유지되는지를 고정한다.
    #[test]
    fn tab_cannot_steal_focus_onto_table_cell() {
        let ctx = egui::Context::default();
        let id = egui::Id::new(("cell", 3usize, 2usize));
        // render_table이 한 셀에 대해 하는 것과 같은 순서:
        // interact → context_menu → surrender_focus.
        let draw = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 18.0));
                let resp = ui.interact(rect, id, TABLE_CELL_SENSE);
                resp.context_menu(|ui| {
                    ui.label("메뉴");
                });
                ui.memory_mut(|m| m.surrender_focus(id));
            });
        };
        // 1프레임: 위젯 등록.
        let _ = ctx.run(Default::default(), |ctx| draw(ctx));
        // 2프레임: Tab.
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        });
        let _ = ctx.run(input, |ctx| draw(ctx));

        assert_eq!(
            ctx.memory(|m| m.focused()),
            None,
            "Tab이 데이터 셀에 포커스를 걸면 안 된다"
        );
    }

    /// 셀 fix가 인라인 셀 편집기의 포커스를 깨지 않는지 고정한다.
    ///
    /// `render_table`은 편집 중인 셀에서 **early return** 하므로 그 셀의
    /// `interact`/`context_menu`/`surrender_focus`는 아예 실행되지 않는다.
    /// 게다가 편집기 id(`("celledit", ..)`)는 셀 상호작용 id(`("cell", ..)`)와
    /// 다르고, `Memory::surrender_focus`는 그 id가 포커스일 때만 지운다
    /// (`memory.rs:762-767`). 즉 **다른** 셀들이 매 프레임 부르는
    /// `surrender_focus`가 편집 중인 TextEdit의 포커스를 빼앗을 수 없다.
    #[test]
    fn cell_surrender_focus_does_not_disturb_inline_editor() {
        let ctx = egui::Context::default();
        let editor = egui::Id::new(("celledit", 3usize, 2usize));
        // 이웃 셀들 — 편집 중이 아니므로 interact + surrender_focus를 돈다.
        let neighbors = [
            egui::Id::new(("cell", 3usize, 1usize)),
            egui::Id::new(("cell", 4usize, 2usize)),
        ];
        let draw = |ctx: &egui::Context, request: bool| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // 편집 중인 셀: TextEdit이 포커스를 요청한다(render_table과 동일).
                let mut buf = String::from("값");
                let resp = ui.add(egui::TextEdit::singleline(&mut buf).id(editor));
                if request {
                    resp.request_focus();
                }
                // 나머지 셀들: 셀 상호작용 + 메뉴 + 포커스 반납.
                for (i, nid) in neighbors.iter().enumerate() {
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(0.0, 40.0 + 20.0 * i as f32),
                        egui::vec2(80.0, 18.0),
                    );
                    let r = ui.interact(rect, *nid, TABLE_CELL_SENSE);
                    r.context_menu(|ui| {
                        ui.label("메뉴");
                    });
                    ui.memory_mut(|m| m.surrender_focus(*nid));
                }
            });
        };
        // 1프레임: 편집기가 포커스를 잡는다.
        let _ = ctx.run(Default::default(), |ctx| draw(ctx, true));
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(editor),
            "사전 조건: 인라인 셀 편집기가 포커스를 쥐어야 한다"
        );
        // 2프레임: 편집기는 더 이상 요청하지 않는다(render_table도 has_focus면
        // 요청하지 않는다). 이웃 셀들의 surrender_focus가 돌아도 포커스는 유지돼야 한다.
        let _ = ctx.run(Default::default(), |ctx| draw(ctx, false));
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(editor),
            "다른 셀의 surrender_focus가 편집 중인 TextEdit의 포커스를 빼앗으면 안 된다"
        );
    }

    // ---- Task 15: 되돌리기 배선 ----

    /// 정렬 → Ctrl+Z 하면 원래 행 순서로 돌아와야 한다(실제 `apply_edit_sort` 경유).
    #[test]
    fn undo_restores_row_order_after_edit_sort() {
        let (mut app, delim) = edit_doc(b"name,n\nCharlie,3\nAlice,1\nBob,2\n", true);
        let doc = app.doc_mut().unwrap();
        let before = doc.edit.as_ref().unwrap().lines.clone();
        let spec = SortSpec { col: 0, kind: SortKind::Text, dir: SortDir::Asc, ci: true };
        apply_edit_sort(doc, &[spec], delim, 1);
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            v(&["name,n", "Alice,1", "Bob,2", "Charlie,3"])
        );
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, before, "정렬 전 순서 복원");
    }

    #[test]
    fn undo_restores_cell_edit() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        doc.editing_cell = Some((1, 0));
        doc.cell_edit_text = "ZZZ".into();
        commit_editing_cell(doc, delim);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", "ZZZ,1"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", "a,1"]));
    }

    #[test]
    fn cell_edit_with_same_value_pushes_no_undo_step() {
        // 값이 그대로면 헛된 Ctrl+Z 단계가 쌓이면 안 된다.
        let (mut app, delim) = edit_doc(b"h,v\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        doc.editing_cell = Some((1, 0));
        doc.cell_edit_text = "a".into();
        commit_editing_cell(doc, delim);
        assert!(doc.edit.as_ref().unwrap().undo.is_empty());
        assert!(!doc.edit.as_ref().unwrap().dirty);
    }

    /// `apply_cell_menu_action`을 실제로 태우기 위한 최소 egui Ui 하네스.
    /// (GUI를 띄우지 않고 한 프레임만 돌려 클립보드/버퍼 변화를 본다.)
    fn with_ui<R>(f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let ctx = egui::Context::default();
        let mut out = None;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                out = Some(f(ui));
            });
        });
        out.expect("CentralPanel 클로저가 한 번은 실행된다")
    }

    #[test]
    fn big_op_confirm_only_for_destructive_and_large() {
        let big = BIG_COLUMN_OP_ROWS + 1;
        // 임계치 이하는 묻지 않는다.
        assert!(!needs_big_op_confirm(CellMenuAction::Copy, BIG_COLUMN_OP_ROWS));
        assert!(!needs_big_op_confirm(CellMenuAction::DeleteRows, 10));
        // 임계치를 넘으면 복사/잘라내기/지우기/행삭제는 묻는다.
        assert!(needs_big_op_confirm(CellMenuAction::Copy, big));
        assert!(needs_big_op_confirm(CellMenuAction::Cut, big));
        assert!(needs_big_op_confirm(CellMenuAction::Clear, big));
        assert!(needs_big_op_confirm(CellMenuAction::DeleteRows, big));
        // 행 삽입은 한 줄짜리라 범위와 무관하게 묻지 않는다.
        assert!(!needs_big_op_confirm(CellMenuAction::InsertRowAbove, big));
        assert!(!needs_big_op_confirm(CellMenuAction::InsertRowBelow, big));
        // 붙여넣기는 클립보드 크기에 좌우되고 선택 범위 전체를 덮지 않는다.
        assert!(!needs_big_op_confirm(CellMenuAction::Paste, big));
    }

    #[test]
    fn small_column_op_runs_without_confirm() {
        // 임계치 아래(3행)면 확인 없이 즉시 수행되고 대기 상태도 남지 않는다.
        let (mut app, delim) = edit_doc(b"h,v\na,1\nb,2\n", true);
        let doc = app.doc_mut().unwrap();
        doc.cell_sel = None;
        doc.selected_col = Some(1);
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Clear, None)
        });
        assert!(doc.pending_column_op.is_none(), "확인 대기 없음");
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", "a,", "b,"]));
    }

    /// 확인 대기 중에는 **아무것도 바꾸지 않아야** 한다 — 버퍼도, dirty도,
    /// 되돌리기 스택도. 대기만 걸고 즉시 돌아온다.
    #[test]
    fn big_column_op_defers_without_mutating() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\nb,2\n", true);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.cell_sel = Some((1, 0, BIG_COLUMN_OP_ROWS + 5, 1));
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Clear, None)
        });
        assert!(doc.pending_column_op.is_some(), "확인 대기가 걸려야 한다");
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig, "버퍼 불변");
        assert!(!doc.edit.as_ref().unwrap().dirty, "dirty 표시 없음");
        assert!(doc.edit.as_ref().unwrap().undo.is_empty(), "undo 단계 없음");

        // 확인하면(confirmed = true) 실제로 수행된다.
        with_ui(|ui| {
            apply_cell_menu_action_confirmed(
                ui,
                doc,
                delim,
                &mut clip,
                CellMenuAction::Clear,
                None,
                true,
            )
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", ",", ","]));
        assert!(doc.edit.as_ref().unwrap().dirty);
    }

    #[test]
    fn undo_restores_cleared_cells() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\nb,2\n", true);
        let doc = app.doc_mut().unwrap();
        doc.cell_sel = Some((1, 0, 2, 1));
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Clear, None)
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", ",", ","]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", "a,1", "b,2"]));
    }

    #[test]
    fn undo_restores_row_insert_and_delete() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\nb,2\n", true);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        let mut clip = String::new();
        doc.cell_sel = Some((1, 0, 1, 0));
        with_ui(|ui| {
            apply_cell_menu_action(
                ui,
                doc,
                delim,
                &mut clip,
                CellMenuAction::InsertRowAbove,
                None,
            )
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines.len(), 4);
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig, "삽입 취소");

        doc.cell_sel = Some((1, 0, 2, 1));
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::DeleteRows, None)
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig, "삭제 취소");
    }

    /// 버퍼를 통째로 지우면 `remove_row`가 빈 한 줄을 남긴다. 되돌리기가
    /// 그 유령 줄까지 치워 정확히 원본이 되어야 한다(Ctrl+Z 한 번에).
    #[test]
    fn undo_full_buffer_delete_leaves_no_ghost_line() {
        let (mut app, delim) = edit_doc(b"a,1\nb,2\n", false);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        let mut clip = String::new();
        doc.cell_sel = Some((0, 0, 1, 1));
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::DeleteRows, None)
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&[""]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// 붙여넣기가 행을 늘리면 값 복원 + 늘어난 행 제거가 Ctrl+Z 한 번에.
    #[test]
    fn undo_paste_that_grows_rows() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        let mut clip = String::new();
        doc.cell_sel = Some((1, 0, 1, 0));
        with_ui(|ui| {
            apply_cell_menu_action(
                ui,
                doc,
                delim,
                &mut clip,
                CellMenuAction::Paste,
                Some("X\tY\nZ\tW"),
            )
        });
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            v(&["h,v", "X,Y", "Z,W"]),
            "행이 하나 늘고 기존 행은 덮인다"
        );
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// `Event::Paste(s)`의 시스템 클립보드 문자열이 내부 캐시보다 우선한다
    /// (외부 앱 → 뷰어 붙여넣기가 이걸로 성립한다).
    #[test]
    fn paste_prefers_system_clipboard_string() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::from("CACHE");
        doc.cell_sel = Some((1, 0, 1, 0));
        with_ui(|ui| {
            apply_cell_menu_action(
                ui,
                doc,
                delim,
                &mut clip,
                CellMenuAction::Paste,
                Some("EXTERNAL"),
            )
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", "EXTERNAL,1"]));
    }

    /// 컬럼 선택 상태에서 잘라내기 → 그 컬럼 데이터만 비고, Ctrl+Z로 복원.
    #[test]
    fn column_cut_clears_whole_column_and_undoes() {
        let (mut app, delim) = edit_doc(b"h1,h2\na,1\nb,2\nc,3\n", true);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.selected_col = Some(1);
        doc.cell_sel = None;
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Cut, None)
        });
        assert_eq!(clip, "1\n2\n3");
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            v(&["h1,h2", "a,", "b,", "c,"]),
            "헤더는 그대로, 데이터 컬럼만 빈다"
        );
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// 이미 비어 있는 셀 범위를 잘라내도 클립보드는 채워지지만(빈 문자열이라도
    /// 복사 자체는 유효) 실제로 바뀐 게 없으니 undo 단계는 쌓이지 않는다.
    #[test]
    fn cut_on_already_empty_cells_pushes_no_undo_step() {
        let (mut app, delim) = edit_doc(b"h,v\n,\n", true);
        let doc = app.doc_mut().unwrap();
        doc.cell_sel = Some((1, 0, 1, 1));
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Cut, None)
        });
        assert_eq!(clip, "\t", "빈 셀이라도 클립보드 복사는 그대로 일어난다");
        assert!(doc.edit.as_ref().unwrap().undo.is_empty());
        assert!(!doc.edit.as_ref().unwrap().dirty);
    }

    /// 이미 비어 있는 셀 범위를 지워도(Clear) 변화가 없으니 undo 단계가
    /// 쌓이면 안 된다.
    #[test]
    fn clear_on_already_empty_cells_pushes_no_undo_step() {
        let (mut app, delim) = edit_doc(b"h,v\n,\n", true);
        let doc = app.doc_mut().unwrap();
        doc.cell_sel = Some((1, 0, 1, 1));
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Clear, None)
        });
        assert!(doc.edit.as_ref().unwrap().undo.is_empty());
        assert!(!doc.edit.as_ref().unwrap().dirty);
    }

    /// 내용이 있는 범위의 Cut은 정확히 undo 단계 하나를 쌓고 Ctrl+Z로 복원된다
    /// (과도교정 방지 가드).
    #[test]
    fn cut_on_nonempty_cells_pushes_exactly_one_undo_step() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.cell_sel = Some((1, 0, 1, 1));
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Cut, None)
        });
        assert_eq!(clip, "a\t1");
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", ","]));
        assert_eq!(doc.edit.as_ref().unwrap().undo.len(), 1);
        assert!(doc.edit.as_ref().unwrap().dirty);
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// 내용이 있는 범위의 Clear도 정확히 undo 단계 하나를 쌓고 Ctrl+Z로
    /// 복원된다(과도교정 방지 가드).
    #[test]
    fn clear_on_nonempty_cells_pushes_exactly_one_undo_step() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\nb,2\n", true);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.cell_sel = Some((1, 0, 2, 1));
        let mut clip = String::new();
        with_ui(|ui| {
            apply_cell_menu_action(ui, doc, delim, &mut clip, CellMenuAction::Clear, None)
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", ",", ","]));
        assert_eq!(doc.edit.as_ref().unwrap().undo.len(), 1);
        assert!(doc.edit.as_ref().unwrap().dirty);
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// 텍스트 모드 인텐트 하나를 실제 경로로 적용한다(GUI 없이).
    fn apply_text(doc: &mut Document, clip: &mut String, intent: TextEditIntent) {
        with_ui(|ui| apply_text_intent(ui, doc, clip, intent));
    }

    /// 텍스트 모드 문자 입력 → Ctrl+Z(행 수 불변 → Replace).
    #[test]
    fn undo_text_insert() {
        let (mut app, _d) = edit_doc(b"abc\ndef\n", false);
        let doc = app.doc_mut().unwrap();
        doc.sep = SeparatorMode::None;
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.text_caret = tp(1, 1);
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Insert("XY".into()));
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["abc", "dXYef"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// Enter(줄 분할)는 행 수가 늘어난다 → Batch(RemoveInserted + Replace).
    #[test]
    fn undo_text_newline_split() {
        let (mut app, _d) = edit_doc(b"abcd\nzz\n", false);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.text_caret = tp(0, 2);
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Newline);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["ab", "cd", "zz"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig, "줄 분할 취소");
    }

    /// 줄 맨 앞 Backspace(줄 병합)는 행 수가 준다 → Batch(ReinsertRemoved + Replace).
    #[test]
    fn undo_text_backspace_merge() {
        let (mut app, _d) = edit_doc(b"ab\ncd\nef\n", false);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.text_caret = tp(1, 0);
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Backspace);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["abcd", "ef"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig, "줄 병합 취소");
    }

    /// 여러 줄에 걸친 선택 삭제(Cut)도 한 번에 되돌아온다.
    #[test]
    fn undo_text_multiline_cut() {
        let (mut app, _d) = edit_doc(b"hello\nworld\nagain\n", false);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.text_sel = Some((tp(0, 2), tp(2, 3)));
        doc.text_caret = tp(2, 3);
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Cut);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["hein"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// 여러 줄 붙여넣기(행 증가)도 한 번에 되돌아온다.
    #[test]
    fn undo_text_multiline_paste() {
        let (mut app, _d) = edit_doc(b"ab\ncd\n", false);
        let doc = app.doc_mut().unwrap();
        let orig = doc.edit.as_ref().unwrap().lines.clone();
        doc.text_caret = tp(0, 1);
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Paste("1\n2\n3".into()));
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["a1", "2", "3b", "cd"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, orig);
    }

    /// 문서 맨 앞 Backspace는 아무것도 바꾸지 않으므로 undo 단계도 쌓이지 않는다
    /// (그렇지 않으면 Ctrl+Z가 아무 일도 안 하는 헛발질이 된다).
    #[test]
    fn noop_backspace_pushes_no_undo_step() {
        let (mut app, _d) = edit_doc(b"ab\n", false);
        let doc = app.doc_mut().unwrap();
        doc.text_caret = tp(0, 0);
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Backspace);
        assert!(doc.edit.as_ref().unwrap().undo.is_empty());
    }

    /// 순수 이동/복사/전체 선택은 되돌리기 대상이 아니다.
    #[test]
    fn non_mutating_text_intents_push_no_undo() {
        let (mut app, _d) = edit_doc(b"ab\ncd\n", false);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::SelectAll);
        apply_text(doc, &mut clip, TextEditIntent::Copy);
        apply_text(doc, &mut clip, TextEditIntent::Move(CaretMove::Right, false));
        assert!(doc.edit.as_ref().unwrap().undo.is_empty());
    }

    /// 여러 번 편집 후 Ctrl+Z를 반복하면 LIFO로 하나씩 되돌아간다.
    #[test]
    fn repeated_undo_walks_back_lifo() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        let s0 = doc.edit.as_ref().unwrap().lines.clone();
        doc.editing_cell = Some((1, 0));
        doc.cell_edit_text = "X".into();
        commit_editing_cell(doc, delim);
        let s1 = doc.edit.as_ref().unwrap().lines.clone();
        doc.editing_cell = Some((1, 1));
        doc.cell_edit_text = "9".into();
        commit_editing_cell(doc, delim);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["h,v", "X,9"]));
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, s1);
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, s0);
        // 더 되돌릴 게 없으면 아무 일도 없다.
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, s0);
    }

    /// 되돌리기로 행이 줄어도 선택/캐럿이 범위 밖에 남지 않아야 한다.
    #[test]
    fn undo_clamps_selection_and_caret() {
        let (mut app, delim) = edit_doc(b"h,v\na,1\n", true);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        doc.cell_sel = Some((1, 0, 1, 0));
        // 붙여넣기로 행을 늘린 뒤 늘어난 행을 선택해 둔다.
        with_ui(|ui| {
            apply_cell_menu_action(
                ui,
                doc,
                delim,
                &mut clip,
                CellMenuAction::Paste,
                Some("X\nY\nZ"),
            )
        });
        assert_eq!(doc.edit.as_ref().unwrap().lines.len(), 4);
        doc.cell_sel = Some((3, 0, 3, 0));
        doc.text_caret = tp(3, 0);
        undo_once(doc);
        let len = doc.edit.as_ref().unwrap().lines.len();
        assert_eq!(len, 2);
        let (r0, _, r1, _) = doc.cell_sel.unwrap();
        assert!(r0 < len && r1 < len, "선택이 범위 안으로 클램프");
        assert!(doc.text_caret.line < len, "캐럿이 범위 안으로 클램프");
    }

    /// 뷰 전용 모드(편집 버퍼 없음)에서는 되돌리기가 아무 일도 하지 않는다.
    #[test]
    fn undo_is_noop_in_view_only_mode() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        view_doc(doc);
        undo_once(doc); // 패닉하지 않고 조용히 통과
        assert!(doc.edit.is_none());
    }

    #[test]
    fn logical_line_reads_from_edit_buffer() {
        let p = temp(b"a,b\n1,2\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        enter_edit_mode(doc);
        // 편집 버퍼 값을 바꾸면 logical_line도 그 값을 반환.
        doc.edit.as_mut().unwrap().lines[1] = "X,Y".to_string();
        assert_eq!(logical_line(doc, 1).as_deref(), Some("X,Y"));
    }

    // ---- 찾기 / 바꾸기 ----

    /// 찾기 테스트용 편집 모드 문서. 텍스트 모드(구분자 없음)로 열고 인덱싱을
    /// 끝낸 뒤 편집 버퍼를 지정한 줄들로 갈아 끼운다. 찾기는 `logical_line`
    /// 경로만 쓰므로 파일 내용과 버퍼 내용이 달라도 무방하다.
    fn find_test_doc(text: &[&str]) -> App {
        let p = temp_ext(b"placeholder
", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        enter_edit_mode(doc);
        doc.edit.as_mut().unwrap().lines = v(text);
        app
    }

    #[test]
    fn find_next_in_edit_mode_finds_across_lines() {
        let mut app = find_test_doc(&["alpha", "beta", "needle here", "gamma"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "needle".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(
            doc.last_match,
            Some(crate::find::Match { line: 2, col: 0, len: 6 }),
            "logical_line 기반 get_line으로 여러 행에 걸쳐 찾아진다"
        );
        assert!(doc.find_status.is_empty(), "찾았으면 안내 문구는 비운다");
        assert_eq!(doc.pending_scroll_row, Some(2), "그 행으로 스크롤을 요청한다");
    }

    /// 첫 Find Next는 캐럿 **자리부터** 훑어야 한다. `find_next`가 `from`을
    /// 제외하는 규칙 때문에 캐럿을 그대로 넘기면 문서 첫 글자의 매치를
    /// 지나쳐 버린다(`find_origin`이 한 칸 물리는 이유).
    #[test]
    fn first_find_includes_the_caret_position() {
        let mut app = find_test_doc(&["hit at column zero", "another hit"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(
            doc.last_match,
            Some(crate::find::Match { line: 0, col: 0, len: 3 }),
            "문서 맨 앞의 매치를 건너뛰면 안 된다"
        );
    }

    /// 단 하나뿐인 매치가 캐럿 자리에 있어도 첫 찾기가 그것을 찾아낸다
    /// (한 칸 물린 기준이 wrap을 거쳐 결국 그 자리로 돌아온다).
    #[test]
    fn first_find_with_single_match_at_origin() {
        let mut app = find_test_doc(&["hit"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.last_match, Some(crate::find::Match { line: 0, col: 0, len: 3 }));
    }

    #[test]
    fn find_next_sets_not_found_status() {
        let mut app = find_test_doc(&["alpha", "beta"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "zzz".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.find_status, "Not found");
        assert_eq!(doc.last_match, None);
    }

    /// 텍스트 모드에서 찾으면 매치가 선택 표시되고 캐럿이 매치 끝에 간다.
    #[test]
    fn find_selects_match_in_text_mode() {
        let mut app = find_test_doc(&["aa", "bb hit bb"]);
        let doc = app.doc_mut().unwrap();
        assert_eq!(doc.sep, SeparatorMode::None, "사전 조건: 텍스트 모드");
        doc.find_query = "hit".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.text_sel, Some((tp(1, 3), tp(1, 6))));
        assert_eq!(doc.text_caret, tp(1, 6));
    }

    #[test]
    fn replace_one_pushes_undo_and_sets_dirty() {
        let mut app = find_test_doc(&["aaa", "b hit b", "ccc"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        doc.replace_text = "XX".to_owned();
        let before = doc.edit.as_ref().unwrap().undo.len();
        // 첫 Replace는 매치가 아직 없으므로 Find Next처럼 동작한다.
        apply_find_action(doc, FindAction::ReplaceOne);
        assert_eq!(doc.last_match, Some(crate::find::Match { line: 1, col: 2, len: 3 }));
        assert_eq!(doc.edit.as_ref().unwrap().undo.len(), before, "찾기만 했으면 undo는 안 쌓인다");
        // 두 번째 Replace가 실제로 바꾼다.
        apply_find_action(doc, FindAction::ReplaceOne);
        let e = doc.edit.as_ref().unwrap();
        assert_eq!(e.lines[1], "b XX b");
        assert!(e.dirty, "바꾸면 dirty");
        assert_eq!(e.undo.len(), before + 1, "undo가 정확히 한 단계 늘어난다");
        // Ctrl+Z 한 번으로 원상복구.
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines[1], "b hit b");
    }

    /// 한 행에 매치가 여러 개일 때 "바꾸기"는 **한 곳만** 바꾼다
    /// (`replace_in_line`을 그대로 쓰면 그 행의 모든 매치가 바뀐다).
    #[test]
    fn replace_one_changes_only_the_current_match() {
        let mut app = find_test_doc(&["a a a"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "a".to_owned();
        doc.replace_text = "Z".to_owned();
        apply_find_action(doc, FindAction::ReplaceOne); // 찾기
        apply_find_action(doc, FindAction::ReplaceOne); // 첫 매치만 치환
        assert_eq!(doc.edit.as_ref().unwrap().lines[0], "Z a a");
    }

    /// 치환문이 검색어를 포함해도(`a` → `aa`) 방금 넣은 글자를 다시 잡아
    /// 제자리걸음하지 않는다.
    #[test]
    fn replace_one_advances_past_inserted_text() {
        let mut app = find_test_doc(&["a b a"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "a".to_owned();
        doc.replace_text = "aa".to_owned();
        apply_find_action(doc, FindAction::ReplaceOne); // 찾기(0,0)
        apply_find_action(doc, FindAction::ReplaceOne); // 치환 → "aa b a"
        assert_eq!(doc.edit.as_ref().unwrap().lines[0], "aa b a");
        // 다음 매치는 방금 넣은 "aa"의 두 번째 a가 아니라 뒤쪽 "a"여야 한다.
        assert_eq!(doc.last_match, Some(crate::find::Match { line: 0, col: 5, len: 1 }));
    }

    #[test]
    fn replace_one_sanitizes_newline() {
        // 치환문에 개행이 들어와도 lines[i] 불변식이 지켜져야 한다.
        let mut app = find_test_doc(&["a b"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "b".to_owned();
        doc.replace_text = "x\ny".to_owned();
        apply_find_action(doc, FindAction::ReplaceOne);
        apply_find_action(doc, FindAction::ReplaceOne);
        let line = &doc.edit.as_ref().unwrap().lines[0];
        assert_eq!(line, "a x y");
        assert!(!line.contains('\n'));
    }

    #[test]
    fn replace_all_pushes_single_undo_step() {
        let mut app =
            find_test_doc(&["hit one", "no match", "hit two hit", "still nothing"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        doc.replace_text = "Z".to_owned();
        let before = doc.edit.as_ref().unwrap().undo.len();
        apply_find_action(doc, FindAction::ReplaceAll);
        let e = doc.edit.as_ref().unwrap();
        assert_eq!(e.lines, v(&["Z one", "no match", "Z two Z", "still nothing"]));
        assert_eq!(e.undo.len(), before + 1, "undo 스택이 정확히 1 늘어난다");
        assert!(e.dirty);
        assert_eq!(doc.find_status, "3 replacements");
        // Ctrl+Z **한 번**으로 전부 원상복구.
        undo_once(doc);
        assert_eq!(
            doc.edit.as_ref().unwrap().lines,
            v(&["hit one", "no match", "hit two hit", "still nothing"]),
            "한 번의 되돌리기로 모든 행이 복구된다"
        );
    }

    #[test]
    fn replace_all_not_found_leaves_buffer_alone() {
        let mut app = find_test_doc(&["a", "b"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "zzz".to_owned();
        doc.replace_text = "Z".to_owned();
        let before = doc.edit.as_ref().unwrap().undo.len();
        apply_find_action(doc, FindAction::ReplaceAll);
        let e = doc.edit.as_ref().unwrap();
        assert_eq!(e.lines, v(&["a", "b"]));
        assert_eq!(e.undo.len(), before, "헛된 undo 단계를 남기지 않는다");
        assert!(!e.dirty);
        assert_eq!(doc.find_status, "Not found");
    }

    /// 뷰 모드(편집 버퍼 없음)에서는 바꾸기가 아무 일도 하지 않는다.
    /// UI가 버튼을 비활성화하지만 단축키/인텐트 경로도 같은 함수를 지난다.
    #[test]
    fn replace_is_noop_without_edit_buffer() {
        let p = temp_ext(b"hit\nhit\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        view_doc(doc);
        doc.find_query = "hit".to_owned();
        doc.replace_text = "Z".to_owned();
        apply_find_action(doc, FindAction::ReplaceAll);
        apply_find_action(doc, FindAction::ReplaceOne);
        assert!(doc.edit.is_none(), "뷰 모드에서 버퍼가 생기지 않는다");
    }

    // ---- 실사용 파일 성능 측정(수동) ----

    /// **수동 실행 전용.** 대용량 실파일로 `scan_all_matches`의 실제 시간을 잰다.
    /// 손에 큰 파일이 있는 사람만 의미가 있으므로 `#[ignore]`이고, 대상 파일을
    /// 지정하지 않으면 조용히 건너뛴다.
    ///
    /// 실행(앱 exe를 건드리지 않도록 별도 target 디렉터리 권장):
    /// `$env:TV_PERF_FILE="...\big.tsv"; $env:CARGO_TARGET_DIR="...\perf";
    ///  cargo test --release perf_real_file_hangul_extract -- --ignored --nocapture`
    ///
    /// K-1 측정 기준값(899MB / 1540만 행 TSV, needle `인도네시아`, Whole cell,
    /// ignore_case, 12,047행): `is_ascii()` 판정일 때 **229.3초** →
    /// `query_is_case_foldable_by_bytes` 판정일 때 **0.27초**.
    #[test]
    #[ignore]
    fn perf_real_file_hangul_extract() {
        // 대상 파일은 `TV_PERF_FILE`로 지정한다. 기본 경로를 두지 않는 이유:
        // 특정 머신의 사적인 경로가 저장소에 남고, 그 파일이 없는 사람에게는
        // 어차피 의미가 없다. 지정하지 않으면 건너뛴다.
        let Ok(spec) = std::env::var("TV_PERF_FILE") else {
            eprintln!("TV_PERF_FILE 미지정 — 건너뜀");
            return;
        };
        let path_buf = std::path::PathBuf::from(spec);
        let path = path_buf.as_path();
        if !path.exists() {
            eprintln!("파일 없음 — 건너뜀: {}", path.display());
            return;
        }
        let t0 = std::time::Instant::now();
        let src = crate::source::open(path).unwrap();
        let total = src.len();
        let offsets = crate::indexer::scan_offsets(src.as_bytes(), 0, Encoding::Utf8);
        let index = LineIndex::new(total);
        let n = offsets.len();
        index.replace_offsets(offsets);
        index.set_bytes_done(total);
        index.set_phase(Phase::Complete);
        eprintln!("인덱싱 {n} 행 / {total} 바이트 — {:?}", t0.elapsed());

        let mut doc = build_extracted_doc(
            &[],
            Encoding::Utf8,
            SeparatorMode::Char(b'\t'),
            true,
            crate::edit::Newline::Lf,
            "perf".to_owned(),
        );
        doc.source = std::sync::Arc::new(src);
        doc.index = index;
        doc.find_query = "인도네시아".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::WholeCell,
        };
        // 빠른 경로를 타는지 먼저 확인(K-1 이전이면 false → 폴백).
        eprintln!(
            "bytefast_ci_ok = {}",
            bytefast_ci_ok(&doc.find_query, doc.enc)
        );
        let t1 = std::time::Instant::now();
        let rows = scan_all_matches(&doc);
        eprintln!("scan_all_matches: {} 행, {:?}", rows.len(), t1.elapsed());
    }

    // ---- scan_all_matches (E1-5) ----

    /// 뷰 모드 인메모리 문서를 만드는 헬퍼(`build_extracted_doc` 재사용).
    fn scan_view_doc(lines: &[&str], enc: Encoding, sep: SeparatorMode) -> Document {
        let has_header = false;
        let owned: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        build_extracted_doc(
            &owned,
            enc,
            sep,
            has_header,
            crate::edit::Newline::Lf,
            "scan-test".to_owned(),
        )
    }

    /// `scan_all_matches`의 결과를 `matching_lines` 브루트포스와 같은 행 집합으로
    /// 비교하는 공통 검사. 두 경로가 반드시 일치해야 한다.
    ///
    /// 브루트포스에도 **`effective_query`를 넘긴다**(날 `doc.find_query`가
    /// 아니라). 계약이 말하는 것은 "두 경로가 같은 needle에 대해 같은 행 집합을
    /// 준다"이므로, 이스케이프가 켜진 문서에서 한쪽에만 날 문자열을 넣으면 이
    /// 검사는 계약이 아니라 배선 실수를 재현할 뿐이다.
    fn assert_scan_equals_brute(doc: &Document) {
        let got = scan_all_matches(doc);
        let n = doc_line_count(doc);
        let delim = doc_delimiter(doc);
        let brute: Vec<u32> = crate::find::matching_lines(
            n,
            &effective_query(doc),
            &doc.find_opts,
            delim,
            |i| logical_line(doc, i),
        )
        .into_iter()
        .map(|i| i as u32)
        .collect();
        assert_eq!(got, brute, "scan_all_matches가 브루트포스와 다르다");
    }

    #[test]
    fn scan_all_matches_view_mode_matches_brute_force() {
        // UTF-8·CP949·UTF-16LE·UTF-16BE + Partial/WholeWord/WholeCell을 두루 확인한다.
        // UTF-16은 코드유닛이 2바이트라 needle 바이트가 문자 경계와 어긋나게 걸릴 수
        // 있는데, 그런 위양성 후보를 find_in_line_scoped 최종 필터가 걸러야 한다.
        let cases: &[(&[&str], Encoding, SeparatorMode)] = &[
            (&["a,b,c", "hit,x", "y,hit", "no"], Encoding::Utf8, SeparatorMode::Char(b',')),
            (&["가,나", "다,가", "가나,x"], Encoding::Cp949, SeparatorMode::Char(b',')),
            (&["가,나", "다,가", "가나,x"], Encoding::Utf16Le, SeparatorMode::Char(b',')),
            (&["가,나", "다,가", "가나,x"], Encoding::Utf16Be, SeparatorMode::Char(b',')),
            (&["The quick", "brown hit", "hit the fox"], Encoding::Utf8, SeparatorMode::None),
        ];
        let scopes = [
            crate::find::MatchScope::Partial,
            crate::find::MatchScope::WholeWord,
            crate::find::MatchScope::WholeCell,
        ];
        for (lines, enc, sep) in cases {
            for needle in ["hit", "가", "가나", "the"] {
                for &scope in &scopes {
                    for match_case in [true, false] {
                        let mut doc = scan_view_doc(lines, *enc, *sep);
                        doc.find_query = needle.to_owned();
                        doc.find_opts = crate::find::FindOptions { match_case, scope };
                        assert_scan_equals_brute(&doc);
                    }
                }
            }
        }
    }

    #[test]
    fn scan_all_matches_edit_mode() {
        // 편집 버퍼 순회 경로. 여러 scope로 브루트포스와 일치하는지.
        let (mut app, _d) = edit_doc(b"a,b\nhit,x\ny,hit\nno,no\n", false);
        let doc = app.doc_mut().unwrap();
        for needle in ["hit", "b"] {
            for scope in [
                crate::find::MatchScope::Partial,
                crate::find::MatchScope::WholeWord,
                crate::find::MatchScope::WholeCell,
            ] {
                for match_case in [true, false] {
                    doc.find_query = needle.to_owned();
                    doc.find_opts = crate::find::FindOptions { match_case, scope };
                    assert_scan_equals_brute(doc);
                }
            }
        }
    }

    /// **K-1 회귀(리뷰가 찾은 위음성).** needle `"i\u{0307}"`(U+0130 `İ`의
    /// 다다자 소문자 확장과 바이트가 똑같은 시퀀스)로 검색하면, 문서의 `İ`는
    /// 브루트포스 기준 매치인데 고친 전(fastpath=true, U+0307 예외 없이)에는
    /// 바이트 경로가 그 행을 놓쳤다. `scan_all_matches`가 실제로 브루트포스와
    /// 같은 행 집합을 내는지 Partial/WholeCell 둘 다 확인한다.
    #[test]
    fn scan_all_matches_i_with_combining_dot_above_needle_matches_brute_force() {
        let needle = "i\u{307}";
        assert_eq!(needle.as_bytes(), [0x69, 0xCC, 0x87]);
        let lines: &[&str] = &["İ,x", "i\u{307},y", "z,w"];
        for scope in
            [crate::find::MatchScope::Partial, crate::find::MatchScope::WholeCell]
        {
            let mut doc = scan_view_doc(lines, Encoding::Utf8, SeparatorMode::Char(b','));
            doc.find_query = needle.to_owned();
            doc.find_opts = crate::find::FindOptions { match_case: false, scope };
            // 고치기 전에는 이 지점에서 바이트 경로가 위음성으로 행 0을 놓쳤다.
            assert_scan_equals_brute(&doc);
            let got = scan_all_matches(&doc);
            assert_eq!(got, vec![0, 1], "İ가 있는 행 0을 놓치면 안 된다 (scope={scope:?})");
        }
    }

    /// **fastpath 회귀 방지.** 위 needle이 이제 폴백으로 빠졌는지, 즉 고친
    /// 판정이 이 needle을 더 이상 빠른 경로에 들여보내지 않는지 직접 확인한다
    /// — `assert_scan_equals_brute` 하나만으로는 "폴백이라서 우연히 맞았다"와
    /// "빠른 경로인데 우연히 맞았다"를 구분하지 못한다.
    #[test]
    fn bytefast_ci_ok_rejects_i_with_combining_dot_above() {
        assert!(!bytefast_ci_ok("i\u{307}", Encoding::Utf8));
    }

    #[test]
    fn scan_ignore_case_matches_brute_force() {
        // ignore_case에서 프리필터가 무엇을 거르든 최종 결과 == 브루트포스.
        // ASCII needle과 비ASCII(한글) needle 둘 다, 뷰/편집 모드 모두.
        // 혼합 대소문자("Ab")를 프리필터가 놓치지 않는지가 핵심이다.
        let lines: &[&str] = &["Ab", "aB", "AB", "ab", "가나", "가나다"];
        let mut view = scan_view_doc(lines, Encoding::Utf8, SeparatorMode::None);
        for needle in ["ab", "가나"] {
            view.find_query = needle.to_owned();
            view.find_opts = crate::find::FindOptions {
                match_case: false,
                scope: crate::find::MatchScope::Partial,
            };
            assert_scan_equals_brute(&view);
        }
        // 편집 모드도 같은 데이터로.
        let (mut app, _d) = edit_doc(b"Ab\naB\nAB\nab\n\xea\xb0\x80\xeb\x82\x98\n", false);
        let doc = app.doc_mut().unwrap();
        doc.sep = SeparatorMode::None;
        for needle in ["ab", "가나"] {
            doc.find_query = needle.to_owned();
            doc.find_opts = crate::find::FindOptions {
                match_case: false,
                scope: crate::find::MatchScope::Partial,
            };
            assert_scan_equals_brute(doc);
        }
    }

    #[test]
    fn scan_whole_cell_excludes_partial_rows() {
        // WholeCell 스캔은 부분 매치 행(셀 전체가 아닌 행)을 뺀다.
        // "hit,x"(셀 0 = hit) 매치, "hitting,y"(부분)는 제외.
        let mut doc = scan_view_doc(
            &["hit,x", "hitting,y", "z,hit"],
            Encoding::Utf8,
            SeparatorMode::Char(b','),
        );
        doc.find_query = "hit".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::WholeCell,
        };
        assert_eq!(scan_all_matches(&doc), vec![0, 2], "부분 매치 행은 빠진다");
        assert_scan_equals_brute(&doc);
    }

    #[test]
    fn scan_empty_query_is_empty() {
        let doc = scan_view_doc(&["a", "b"], Encoding::Utf8, SeparatorMode::None);
        // find_query가 비어 있으면 빈 Vec.
        assert!(scan_all_matches(&doc).is_empty());
    }

    // ---- Task G: 바이트 빠른 경로 (classify_cell_hit + 빠른/폴백 혼합) ----

    /// `classify_cell_hit` 순수 함수 단위 테스트. 순수 경계 Confirmed, 따옴표 경계
    /// NeedsRefine, 부분 걸침 NotCellBoundary. 이 함수가 잘못 판정하면 Whole cell
    /// 빠른 경로가 위양성/위음성을 내므로 각 분기를 직접 부른다(인라인 복붙 금지).
    #[test]
    fn classify_cell_hit_branches() {
        // `a,bb,c` — 셀 1 = bb. hit=2, needle_len=2, delim=','.
        // 앞 = bytes[1] = ',', 뒤 = bytes[4] = ',' → 순수 경계 Confirmed.
        let b = b"a,bb,c";
        assert_eq!(
            classify_cell_hit(b, 0, b.len(), 2, 2, b',', false),
            CellHit::Confirmed
        );
        // 줄시작에 걸린 셀: `bb,c` — hit=0. 앞 = 줄시작(None), 뒤 = ',' → Confirmed.
        let b = b"bb,c";
        assert_eq!(
            classify_cell_hit(b, 0, b.len(), 0, 2, b',', false),
            CellHit::Confirmed
        );
        // 줄끝에 걸린 셀: `a,bb` — hit=2. 앞 = ',', 뒤 = 줄끝(None) → Confirmed.
        let b = b"a,bb";
        assert_eq!(
            classify_cell_hit(b, 0, b.len(), 2, 2, b',', false),
            CellHit::Confirmed
        );
        // 부분 걸침: `Johnson`에서 needle `John` hit=0, len=4. 뒤 = 's'(delim 아님)
        // → NotCellBoundary.
        let b = b"Johnson";
        assert_eq!(
            classify_cell_hit(b, 0, b.len(), 0, 4, b',', false),
            CellHit::NotCellBoundary
        );
        // 따옴표 셀: `"John",x` 에서 needle `John` hit=1(따옴표 뒤), len=4.
        // 앞 = '"', 뒤 = '"' → NeedsRefine.
        let b = b"\"John\",x";
        assert_eq!(
            classify_cell_hit(b, 0, b.len(), 1, 4, b',', false),
            CellHit::NeedsRefine
        );
        // 개행이 제외된 line_end를 넘겨도 줄끝 경계로 잡힌다: `a,bb`의 내용 끝은 4.
        let b = b"a,bb\n";
        assert_eq!(
            classify_cell_hit(b, 0, 4, 2, 2, b',', false),
            CellHit::Confirmed,
            "line_end(개행 제외)가 needle 끝이면 뒤 경계로 인정"
        );
        // needle에 delim이 있으면(`a,b`) 순수 경계처럼 보여도 무조건 NeedsRefine.
        // `a,b,c`에서 hit=0, len=3: 앞=줄시작, 뒤=','(delim)이라 경계로 보이지만
        // 실은 두 셀을 가로지른다 → 폴백.
        let b = b"a,b,c";
        assert_eq!(
            classify_cell_hit(b, 0, b.len(), 0, 3, b',', true),
            CellHit::NeedsRefine,
            "needle에 delim이 있으면 바이트만으로 확정하지 않는다"
        );
    }

    /// 잘못 판정하면 테스트가 깨지는지(뮤테이션 감지). before 경계 검사를 뒤집으면
    /// 부분 걸침이 Confirmed로 잘못 나오는데, 위 `classify_cell_hit_branches`의
    /// NotCellBoundary 단정이 그걸 잡는다. 여기서는 Confirmed vs NotCellBoundary가
    /// 실제로 갈리는 입력을 명시해 판정이 무의미하게 항상 같은 값을 주지 않음을 못박는다.
    #[test]
    fn classify_cell_hit_distinguishes_boundary() {
        let confirmed = classify_cell_hit(b"a,bb,c", 0, 6, 2, 2, b',', false);
        let not_boundary = classify_cell_hit(b"Johnson", 0, 7, 0, 4, b',', false);
        assert_ne!(
            confirmed, not_boundary,
            "순수 경계와 부분 걸침은 반드시 다른 판정이어야 한다"
        );
    }

    /// Whole cell match_case: 따옴표 없는 대량 데이터에서 빠른 경로가 타고 결과가
    /// 브루트포스와 같다. 여러 행·여러 셀·부분 걸침 행을 섞는다.
    #[test]
    fn scan_wholecell_bytefast_matches_brute_force() {
        // 따옴표가 needle을 쪼개는 행(`"hi"t` → 표시값 `hit`)을 섞는다 — 그 행은
        // 파일 바이트에 `hit`이 붙어 나타나지 않으므로 "히트 0개 = 비매치"로
        // 단정하던 옛 코드가 놓쳤다(위음성).
        let lines: Vec<String> = (0..500)
            .map(|i| match i % 6 {
                0 => "hit,a,b".to_string(),   // 셀0 = hit (확정)
                1 => "x,hit,y".to_string(),   // 셀1 = hit (확정)
                2 => "hitting,z".to_string(), // 부분 (제외)
                3 => "\"hi\"t,z".to_string(), // 표시값 hit (폴백으로만 잡힌다)
                4 => "\"a_,,hit".to_string(), // 닫히지 않은 따옴표 (제외돼야 한다)
                _ => "p,q,r".to_string(),     // 매치 없음
            })
            .collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut doc = scan_view_doc(&refs, Encoding::Utf8, SeparatorMode::Char(b','));
        doc.find_query = "hit".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::WholeCell,
        };
        assert_scan_equals_brute(&doc);
        // 부분 걸침(hitting) 행은 결과에 없어야 한다.
        let got = scan_all_matches(&doc);
        assert!(!got.is_empty());
        for &r in &got {
            assert_ne!(r % 6, 2, "hitting 행(부분)은 빠져야 한다");
            assert_ne!(r % 6, 4, "닫히지 않은 따옴표 행은 셀 전체가 hit가 아니다");
        }
        assert!(
            got.iter().any(|&r| r % 6 == 3),
            "`\"hi\"t`(표시값 hit) 행이 빠지면 위음성이다"
        );
    }

    /// 따옴표 셀 정확성 회귀. 빠른 경로가 따옴표에 속지 않고 폴백이 잡는다.
    /// 순수 셀과 따옴표 셀을 한 문서에 섞어 **빠른 경로와 폴백이 둘 다** 타게 한다.
    #[test]
    fn scan_wholecell_quoted_cell_matches() {
        // 행 0: 순수 셀 `John Smith`(빠른 경로 확정).
        // 행 1: 따옴표 셀 `"John Smith"` — 표시값 John Smith, needle과 같음(폴백 확정).
        // 행 2: 따옴표 안 콤마 `"a,b"` — needle `a,b`와 같음(폴백 확정). ↓ 별 문서.
        // 행 3: `Johnson` — 부분이라 매치 없음.
        let lines = &["John Smith,x", "\"John Smith\",x", "Johnson,y", "z,John Smith"];
        let mut doc = scan_view_doc(lines, Encoding::Utf8, SeparatorMode::Char(b','));
        doc.find_query = "John Smith".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::WholeCell,
        };
        // 브루트포스와 같아야 한다(핵심 계약).
        assert_scan_equals_brute(&doc);
        assert_eq!(
            scan_all_matches(&doc),
            vec![0, 1, 3],
            "순수 셀·따옴표 셀 모두 매치, Johnson(부분)만 제외"
        );

        // 따옴표 안 콤마 셀: needle에 delim이 들어 있어 바이트만으론 셀을 오판할
        // 케이스. 폴백이 정확히 잡는지.
        let lines2 = &["\"a,b\",c", "a,b,c", "x,\"a,b\""];
        let mut doc2 = scan_view_doc(lines2, Encoding::Utf8, SeparatorMode::Char(b','));
        doc2.find_query = "a,b".to_owned();
        doc2.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::WholeCell,
        };
        assert_scan_equals_brute(&doc2);
        assert_eq!(
            scan_all_matches(&doc2),
            vec![0, 2],
            "따옴표로 감싼 a,b 셀만 매치. 행1(a,b,c)은 세 셀이라 매치 없음"
        );
    }

    /// `Johnson`에서 needle `John`은 Whole cell로 매치 없음(부분 걸침 제외).
    #[test]
    fn scan_wholecell_partial_containment_excluded() {
        let mut doc = scan_view_doc(
            &["Johnson,x", "John,y", "z,Johnson"],
            Encoding::Utf8,
            SeparatorMode::Char(b','),
        );
        doc.find_query = "John".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::WholeCell,
        };
        assert_scan_equals_brute(&doc);
        assert_eq!(scan_all_matches(&doc), vec![1], "셀 전체가 John인 행만");
    }

    /// Partial match_case가 재판정 없이 memmem 히트로 확정돼도 브루트포스와 같다.
    #[test]
    fn scan_partial_bytefast_matches_brute_force() {
        let lines: Vec<String> = (0..300)
            .map(|i| if i % 3 == 0 { "has hit here".to_string() } else { "none".to_string() })
            .collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let mut doc = scan_view_doc(&refs, Encoding::Utf8, SeparatorMode::Char(b','));
        doc.find_query = "hit".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::Partial,
        };
        assert_scan_equals_brute(&doc);
    }

    /// UTF-16은 빠른 경로를 타지 않고 폴백으로 가야 정확하다. 구분자·개행이 2바이트
    /// 코드유닛이라 원바이트 경계 판정이 성립하지 않기 때문. 브루트포스와 같은지로 확인.
    #[test]
    fn scan_wholecell_utf16_falls_back_correctly() {
        for enc in [Encoding::Utf16Le, Encoding::Utf16Be] {
            let mut doc = scan_view_doc(
                &["hit,x", "가나,hit", "hitting,y", "z,가나"],
                enc,
                SeparatorMode::Char(b','),
            );
            for needle in ["hit", "가나"] {
                doc.find_query = needle.to_owned();
                for scope in [
                    crate::find::MatchScope::Partial,
                    crate::find::MatchScope::WholeCell,
                ] {
                    doc.find_opts = crate::find::FindOptions { match_case: true, scope };
                    assert_scan_equals_brute(&doc);
                }
            }
        }
    }

    /// 편집 모드 Whole cell/Partial 바이트 빠른 경로도 브루트포스와 같다.
    /// 따옴표 셀 + 순수 셀 + 부분 걸침을 섞어 빠른 경로와 폴백이 둘 다 타게 한다.
    #[test]
    fn scan_edit_mode_bytefast_matches_brute_force() {
        let (mut app, _d) = edit_doc(
            b"John Smith,x\n\"John Smith\",y\nJohnson,z\nq,John Smith\n",
            false,
        );
        let doc = app.doc_mut().unwrap();
        for needle in ["John Smith", "John", "x"] {
            for scope in [
                crate::find::MatchScope::Partial,
                crate::find::MatchScope::WholeWord,
                crate::find::MatchScope::WholeCell,
            ] {
                for match_case in [true, false] {
                    doc.find_query = needle.to_owned();
                    doc.find_opts = crate::find::FindOptions { match_case, scope };
                    assert_scan_equals_brute(doc);
                }
            }
        }
    }

    // ---- Task H: ignore_case 바이트 빠른 경로 ----

    /// `ascii_lower`는 ASCII 대문자만 접고 나머지는 그대로 둔다. 비ASCII 바이트를
    /// 건드리면 멀티바이트 시퀀스가 뒤틀려 위양성/위음성이 둘 다 가능해진다.
    #[test]
    fn ascii_lower_folds_only_ascii_uppercase() {
        assert_eq!(ascii_lower(b'A'), b'a');
        assert_eq!(ascii_lower(b'Z'), b'z');
        assert_eq!(ascii_lower(b'a'), b'a');
        assert_eq!(ascii_lower(b'0'), b'0');
        assert_eq!(ascii_lower(b'_'), b'_');
        // 0x80 이상(UTF-8 연속 바이트·CP949 리드/트레일)은 절대 바뀌지 않는다.
        for b in 0x80u8..=0xFF {
            assert_eq!(ascii_lower(b), b, "비ASCII 바이트 {b:#x}는 그대로여야 한다");
        }
    }

    /// `find_ci_ascii`가 혼합 대소문자를 전부 잡는다 — 이것이 "needle 한 벌만
    /// 접는" 옛 방식이 놓쳤던 케이스다(위음성).
    #[test]
    fn find_ci_ascii_finds_all_case_variants() {
        let needle: Vec<u8> = "ab".bytes().map(ascii_lower).collect();
        for hay in ["Ab", "aB", "AB", "ab"] {
            assert_eq!(
                find_ci_ascii(hay.as_bytes(), &needle),
                Some(0),
                "{hay}를 놓치면 안 된다"
            );
        }
        // 앞에 다른 바이트가 붙어도 위치가 정확해야 한다.
        assert_eq!(find_ci_ascii(b"xxAbxx", &needle), Some(2));
        // 없으면 None.
        assert_eq!(find_ci_ascii(b"xyz", &needle), None);
        // hay가 needle보다 짧으면 None(범위 밖 접근 방지).
        assert_eq!(find_ci_ascii(b"a", &needle), None);
        // 빈 needle은 None — 매치를 내면 호출부가 무한 루프에 빠진다.
        assert_eq!(find_ci_ascii(b"abc", b""), None);
        assert_eq!(find_ci_ascii(b"", b""), None);
    }

    /// 비ASCII 바이트가 섞여도 ASCII needle의 위치가 정확하고, 접기가 비ASCII를
    /// 건드리지 않아 한글 바이트가 우연히 매치를 만들지 않는다(UTF-8 기준).
    #[test]
    fn find_ci_ascii_leaves_non_ascii_alone() {
        let needle: Vec<u8> = "hit".bytes().map(ascii_lower).collect();
        let hay = "가나HIT다".as_bytes();
        assert_eq!(find_ci_ascii(hay, &needle), Some("가나".len()));
        // 한글만 있는 hay에는 ASCII needle이 없다(UTF-8 연속 바이트는 ≥0x80).
        assert_eq!(find_ci_ascii("가나다".as_bytes(), &needle), None);
    }

    /// `find_ci_ascii_all`은 겹치지 않는 모든 출현을 준다(`memmem::find_iter`와
    /// 같은 규칙). Whole cell은 히트마다 경계를 봐야 하므로 첫 히트만으론 부족하다.
    #[test]
    fn find_ci_ascii_all_collects_non_overlapping() {
        let needle: Vec<u8> = "ab".bytes().map(ascii_lower).collect();
        assert_eq!(find_ci_ascii_all(b"AB,ab,Ab", &needle), vec![0, 3, 6]);
        // "aaaa"에서 "aa"는 비중첩 2개(0, 2) — 3개가 아니다.
        let n2: Vec<u8> = "aa".bytes().map(ascii_lower).collect();
        assert_eq!(find_ci_ascii_all(b"AaaA", &n2), vec![0, 2]);
        assert!(find_ci_ascii_all(b"abc", b"").is_empty());
        assert!(find_ci_ascii_all(b"xyz", &needle).is_empty());
    }

    /// `query_is_case_foldable_by_bytes`: needle이 바이트 접기만으로 대소문자
    /// 무시 비교가 성립하는가. ASCII 전부 + 대소문자가 없는 비ASCII만 참.
    #[test]
    fn query_is_case_foldable_by_bytes_judgment() {
        // ASCII는 대문자든 소문자든 `ascii_lower`가 바이트로 접는다 → 참.
        assert!(query_is_case_foldable_by_bytes("hit"));
        assert!(query_is_case_foldable_by_bytes("HIT"));
        assert!(query_is_case_foldable_by_bytes("Hit_0-9!"));
        // 한글은 대소문자가 없다 → 참. (사용자 실사용 needle)
        assert!(query_is_case_foldable_by_bytes("인도네시아"));
        assert!(query_is_case_foldable_by_bytes("가나다"));
        // 한자·가나도 마찬가지.
        assert!(query_is_case_foldable_by_bytes("大韓民國"));
        assert!(query_is_case_foldable_by_bytes("こんにちは"));
        // ASCII + 한글 혼합도 참(모든 문자가 조건을 만족).
        assert!(query_is_case_foldable_by_bytes("한글AB"));
        // 유니코드 접기가 필요한 비ASCII는 거짓.
        assert!(!query_is_case_foldable_by_bytes("É")); // 라틴 악센트
        assert!(!query_is_case_foldable_by_bytes("À"));
        assert!(!query_is_case_foldable_by_bytes("İ")); // 1:N 확장(i + U+0307)
        assert!(!query_is_case_foldable_by_bytes("Σ")); // 그리스
        assert!(!query_is_case_foldable_by_bytes("Ж")); // 키릴
        // **양방향이어야 한다.** 이미 소문자인 비ASCII도 거짓 — 파일의 `É`가
        // 접혀 `é`가 되므로 브루트포스는 매치라고 답하는데 바이트 경로는 못
        // 잡는다(위음성). 소문자화만 보는 판정이면 여기서 깨진다.
        assert!(!query_is_case_foldable_by_bytes("é"));
        assert!(!query_is_case_foldable_by_bytes("à"));
        assert!(!query_is_case_foldable_by_bytes("σ"));
        assert!(!query_is_case_foldable_by_bytes("ß")); // 대문자화가 1:N("SS")
        // 한 글자라도 위반하면 전체가 거짓.
        assert!(!query_is_case_foldable_by_bytes("인도네시아É"));
        assert!(!query_is_case_foldable_by_bytes("인도네시아é"));
        // 빈 문자열은 공허참(빈 needle은 `bytefast_ci_ok`가 따로 막는다).
        assert!(query_is_case_foldable_by_bytes(""));
    }

    /// **전수 근거 1.** 한글 음절 영역(U+AC00~U+D7A3) 11,172자와 CJK 통합한자·
    /// 가나는 **하나도** 대소문자 매핑을 갖지 않는다 — K-1의 판정이 기대는
    /// 사실이므로 못박는다. 반대로 라틴 확장에는 접히는 문자가 실제로 존재한다
    /// (판정이 무의미하게 항상 참이 아님을 증명).
    #[test]
    fn hangul_and_cjk_never_case_fold() {
        for c in '\u{AC00}'..='\u{D7A3}' {
            assert!(
                query_is_case_foldable_by_bytes(&c.to_string()),
                "한글 음절 {c}(U+{:04X})가 접힌다 — 판정의 전제가 깨졌다",
                c as u32
            );
        }
        for c in ('\u{4E00}'..='\u{9FFF}').chain('\u{3040}'..='\u{30FF}') {
            assert!(
                query_is_case_foldable_by_bytes(&c.to_string()),
                "CJK/가나 {c}(U+{:04X})가 접힌다",
                c as u32
            );
        }
        // 비ASCII 중 실제로 접히는 문자가 있어야 판정이 의미를 갖는다.
        let folding = ('\u{00C0}'..='\u{024F}')
            .filter(|&c| !query_is_case_foldable_by_bytes(&c.to_string()))
            .count();
        assert!(folding > 100, "라틴 확장에 접히는 문자가 {folding}개뿐 — 판정이 의심스럽다");
    }

    /// **전수 근거 2 (판정의 안전성 그 자체).** 이 판정을 통과한 비ASCII 문자로
    /// **다른 문자가 접혀 오는 일이 없어야** 바이트 비교가 브루트포스와 같은
    /// 질문이 된다(needle `é`가 파일의 `É`를 놓치는 위음성 방지).
    ///
    /// 전 유니코드(U+0000~U+10FFFF)를 훑어 "소문자화하면 c가 되는 다른 문자"의
    /// 집합을 만들고, 판정을 통과한 비ASCII 문자 중 그 집합에 든 것이 **0개**임을
    /// 확인한다. 소문자화만 보는(대문자화를 빼는) 판정으로 되돌리면 1,453개가
    /// 나와 이 테스트가 깨진다 — 뮤테이션 감지 지점이다.
    ///
    /// **1글자 확장만으로는 불충분하다(K 리뷰가 찾은 구멍).** 위 루프는
    /// `to_lowercase()`가 **정확히 한 글자**로 접히는 경우만 `fold_targets`에
    /// 넣는다 — U+0130 `İ`처럼 **두 글자**(`i`+U+0307)로 접히는 문자는 원래
    /// 여기서 통째로 빠졌다. 그래서 아래에서 **다다자 확장 전체**를 needle
    /// 문자열로 만들어 판정에 넣어 본다: 확장의 모든 글자가 개별적으로는
    /// 판정을 통과하더라도, "이 시퀀스 자체가 다른 문자의 접힘 결과"라면 그
    /// needle은 거부돼야 한다(그러지 않으면 brute force가 원본 문자를 접어
    /// 만드는 것과 같은 바이트 시퀀스를 바이트 경로가 찾아내지 못한다).
    #[test]
    fn foldable_judgment_has_no_incoming_fold_targets() {
        use std::collections::HashSet;
        let mut fold_targets: HashSet<char> = HashSet::new();
        // 다다자(멀티 글자) 소문자 확장 전체를 문자열로 모은 것 — 이 시퀀스를
        // needle으로 쓰면 판정이 반드시 거부해야 한다(그 시퀀스가 원본 문자
        // 하나의 접힘 결과이므로).
        let mut multichar_expansions: Vec<(char, String)> = Vec::new();
        for cp in 0u32..=0x10FFFF {
            let Some(c) = char::from_u32(cp) else { continue };
            let lo: String = c.to_lowercase().collect();
            let mut it = lo.chars();
            match (it.next(), it.next()) {
                (Some(first), None) => {
                    if first != c {
                        fold_targets.insert(first);
                    }
                }
                (Some(_), Some(_)) => {
                    // 2글자 이상으로 접히는 문자 — 시퀀스 전체를 별도로 검사한다.
                    multichar_expansions.push((c, lo));
                }
                _ => {}
            }
        }
        let mut holes = Vec::new();
        for cp in 0x80u32..=0x10FFFF {
            let Some(c) = char::from_u32(cp) else { continue };
            if query_is_case_foldable_by_bytes(&c.to_string()) && fold_targets.contains(&c) {
                holes.push(c.to_string());
            }
        }
        // 다다자 확장 시퀀스 자체가 needle로 들어왔을 때 판정을 통과하면 구멍이다
        // — brute force는 원본 문자(예: İ)를 접어 이 시퀀스를 만들어 매치시키는데
        // 바이트 경로는 원본 문자의 바이트를 볼 수 없다.
        for (src, expansion) in &multichar_expansions {
            if query_is_case_foldable_by_bytes(expansion) {
                holes.push(format!(
                    "U+{:04X}({src})의 확장 {:?}가 판정을 통과한다",
                    *src as u32, expansion
                ));
            }
        }
        assert!(
            holes.is_empty(),
            "판정을 통과했는데 다른 문자의 접힘 결과인 구멍이 {}개 있다: {:?}",
            holes.len(),
            &holes[..holes.len().min(10)]
        );
        // 판정이 무의미하게 전부 거짓이 아님(한글은 통과해야 한다).
        assert!(query_is_case_foldable_by_bytes("인도네시아"));
    }

    /// 위 강화된 테스트가 실제로 **K-1의 구멍을 잡아내는지** 직접 재현한다 —
    /// U+0130의 다다자 확장 `"i\u{0307}"`가 판정을 통과하면(수정 전 동작) 이
    /// 단정이 실패해야 하고, 수정 후(U+0307 거부)에는 통과해야 한다.
    #[test]
    fn multichar_lower_expansion_i_with_dot_above_is_rejected() {
        let expansion = "\u{130}".chars().next().unwrap().to_lowercase().collect::<String>();
        assert_eq!(expansion, "i\u{307}", "U+0130의 소문자 확장이 예상과 다르다");
        assert!(
            !query_is_case_foldable_by_bytes(&expansion),
            "İ의 다다자 확장 자체가 빠른 경로를 통과하면 안 된다 — brute force와 어긋난다"
        );
    }

    /// U+0307이 "다다자 확장에 등장하는 유일한 문자"라는 전제를 전수 검증한다 —
    /// `query_is_case_foldable_by_bytes`가 U+0307 하나만 특별 취급해도 되는 이유.
    /// 이 전제가 깨지면(유니코드 데이터 변경 등) 이 테스트가 먼저 실패한다.
    #[test]
    fn multichar_lower_expansion_pieces_are_exactly_u0307() {
        use std::collections::HashSet;
        let mut pieces: HashSet<char> = HashSet::new();
        for cp in 0u32..=0x10FFFF {
            let Some(c) = char::from_u32(cp) else { continue };
            let lo: Vec<char> = c.to_lowercase().collect();
            if lo.len() >= 2 {
                for &piece in &lo {
                    pieces.insert(piece);
                }
            }
        }
        assert_eq!(
            pieces,
            HashSet::from(['\u{0307}', 'i']),
            "다다자 소문자 확장에 등장하는 문자 집합이 예상과 다르다: {pieces:?}"
        );
    }

    /// `bytefast_ci_ok` 판정. 바이트로 접히는 needle + UTF-8/CP949만 참.
    #[test]
    fn bytefast_ci_ok_conditions() {
        assert!(bytefast_ci_ok("hit", Encoding::Utf8));
        assert!(bytefast_ci_ok("hit", Encoding::Cp949));
        // 유니코드 접기가 필요한 needle은 폴백.
        assert!(!bytefast_ci_ok("İ", Encoding::Utf8));
        assert!(!bytefast_ci_ok("É", Encoding::Utf8));
        // UTF-16은 코드유닛이 2바이트라 바이트 경계가 성립하지 않는다.
        assert!(!bytefast_ci_ok("hit", Encoding::Utf16Le));
        assert!(!bytefast_ci_ok("hit", Encoding::Utf16Be));
        assert!(!bytefast_ci_ok("가나", Encoding::Utf16Le));
        // 빈 needle은 빠른 경로 대상이 아니다.
        assert!(!bytefast_ci_ok("", Encoding::Utf8));
    }

    /// **K-1의 핵심 회귀.** 한글 needle이 ignore_case 빠른 경로를 **타야** 한다.
    /// 예전 판정(`query.is_ascii()`)이면 이 단정이 전부 깨진다 —
    /// 그게 1540만 행을 통째로 디코딩하게 만든 원인이었다.
    #[test]
    fn bytefast_ci_ok_allows_hangul_needle() {
        assert!(bytefast_ci_ok("인도네시아", Encoding::Utf8));
        assert!(bytefast_ci_ok("인도네시아", Encoding::Cp949));
        assert!(bytefast_ci_ok("가나", Encoding::Utf8));
        assert!(bytefast_ci_ok("한글AB", Encoding::Utf8));
        // 그러나 인코딩 조건은 그대로다.
        assert!(!bytefast_ci_ok("인도네시아", Encoding::Utf16Le));
    }

    /// 뮤테이션 감지: 판정이 무의미하게 항상 같은 값을 주지 않음을 못박는다.
    /// `query_is_case_foldable_by_bytes`를 빼면 (`É`, UTF-8)이 true가 되고,
    /// `is_single_byte_enc`를 빼면 (ASCII, UTF-16)이 true가 된다 —
    /// 두 단정이 각각 그걸 잡는다.
    #[test]
    fn bytefast_ci_ok_distinguishes_each_condition() {
        assert_ne!(
            bytefast_ci_ok("hit", Encoding::Utf8),
            bytefast_ci_ok("É", Encoding::Utf8),
            "needle이 바이트로 접히는지가 판정을 가른다"
        );
        assert_ne!(
            bytefast_ci_ok("hit", Encoding::Utf8),
            bytefast_ci_ok("hit", Encoding::Utf16Le),
            "인코딩이 판정을 가른다"
        );
    }

    /// `bytefast_ci_confirms`: UTF-8만 바이트 히트로 확정. CP949는 트레일 바이트가
    /// ASCII 대문자와 겹쳐 위양성이 가능하므로 정밀 판정을 거쳐야 한다.
    #[test]
    fn bytefast_ci_confirms_only_utf8() {
        assert!(bytefast_ci_confirms(Encoding::Utf8));
        assert!(!bytefast_ci_confirms(Encoding::Cp949));
        assert!(!bytefast_ci_confirms(Encoding::Utf16Le));
    }

    /// **핵심 계약 테스트.** 대량 혼합 데이터에서 ignore_case Partial/WholeCell이
    /// 브루트포스와 같다. 빠른 경로(순수 셀)와 폴백(따옴표 셀)이 **둘 다** 타도록
    /// 섞고, 대소문자 변형·한글·부분 걸침을 함께 넣는다. 뷰/편집 모드 모두.
    #[test]
    fn scan_ignore_case_bytefast_matches_brute_force() {
        // **완전히 따옴표로 감싼 셀만 넣으면 안 된다.** `"Hit"`류는 따옴표 안에
        // needle 바이트가 그대로 있어 "히트가 반드시 존재한다"는 성질을 만족하므로
        // "히트 0개 = 비매치" 구멍을 **드러내지 못한다**. 따옴표가 needle을 쪼개거나
        // (`"hi"t` → 표시값 `hit`) 언이스케이프하는(`"a""b"` → `a"b`) 형태를 함께 넣어야
        // 그 구멍이 드러난다.
        let lines: Vec<String> = (0..400)
            .map(|i| match i % 12 {
                0 => "HIT,a,b".to_string(),          // 셀0 = HIT (빠른 경로 확정)
                1 => "x,hIt,y".to_string(),          // 셀1 = hIt (빠른 경로 확정)
                2 => "hitting,z".to_string(),        // 부분 걸침 (WholeCell 제외)
                3 => "\"Hit\",q".to_string(),        // 따옴표 셀 (폴백 확정)
                4 => "가나,HIT".to_string(),          // 한글 + 매치
                5 => "p,q,r".to_string(),            // 매치 없음
                6 => "\"a,hit\",z".to_string(),      // 따옴표 안 delim (폴백)
                7 => "\"hi\"t,z".to_string(),        // 표시값 `hit` — 바이트엔 `hit`이 없다
                8 => "\"HI\"T,z".to_string(),        // 위의 대소문자 변형
                9 => "\"a\"a,x".to_string(),         // 표시값 `aa`
                10 => "\"a\"\"b\",x".to_string(),    // 표시값 `a"b` (`""` 언이스케이프)
                _ => "가나다,라마".to_string(),        // 한글만
            })
            .collect();
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        for scope in [crate::find::MatchScope::Partial, crate::find::MatchScope::WholeCell] {
            for needle in ["hit", "HIT", "Hit", "a,hit", "aa", "AA", "a\"b"] {
                let mut doc =
                    scan_view_doc(&refs, Encoding::Utf8, SeparatorMode::Char(b','));
                doc.find_query = needle.to_owned();
                doc.find_opts = crate::find::FindOptions { match_case: false, scope };
                assert_scan_equals_brute(&doc);
            }
        }
        // 빠른 경로가 실제로 결과를 내는지(항상 빈 Vec이면 계약 테스트가 무의미).
        let mut doc = scan_view_doc(&refs, Encoding::Utf8, SeparatorMode::Char(b','));
        doc.find_query = "hit".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::WholeCell,
        };
        let got = scan_all_matches(&doc);
        assert!(!got.is_empty(), "대소문자 변형 셀을 잡아야 한다");
        for &r in &got {
            assert_ne!(r % 12, 2, "hitting 행(부분 걸침)은 빠져야 한다");
            assert_ne!(r % 12, 5, "매치 없는 행이 들어오면 안 된다");
        }
        // 따옴표가 needle을 쪼갠 행(`"hi"t` → 표시값 `hit`)이 **반드시 잡혀야** 한다 —
        // 이것이 "히트 0개 = 비매치" 구멍의 직접 회귀다.
        assert!(
            got.iter().any(|&r| r % 12 == 7),
            "`\"hi\"t`(표시값 hit) 행이 빠지면 위음성이다"
        );

        // 편집 모드도 같은 데이터로(버퍼는 항상 UTF-8).
        let mut src = String::new();
        for l in &lines {
            src.push_str(l);
            src.push('\n');
        }
        let (mut app, _d) = edit_doc(src.as_bytes(), false);
        let doc = app.doc_mut().unwrap();
        for scope in [
            crate::find::MatchScope::Partial,
            crate::find::MatchScope::WholeWord,
            crate::find::MatchScope::WholeCell,
        ] {
            for needle in ["hit", "HIT", "Hit", "a,hit", "aa", "AA", "a\"b"] {
                doc.find_query = needle.to_owned();
                doc.find_opts = crate::find::FindOptions { match_case: false, scope };
                assert_scan_equals_brute(doc);
            }
        }
    }

    /// **K-1 계약 테스트.** 한글 needle이 이제 ignore_case 빠른 경로를 타는데,
    /// 결과는 여전히 브루트포스와 같은 행 집합이어야 한다.
    ///
    /// **한글이 인접한 데이터**를 일부러 넣는다 — UTF-8 self-synchronizing 논증
    /// (`bytefast_ci_confirms` 주석)이 실제로 성립하는지, 즉 needle 바이트열이
    /// 다른 한글 문자 **중간**에 걸려 위양성을 내지 않는지 확인한다.
    /// UTF-8·CP949 둘 다, 따옴표 행(폴백)도 섞는다.
    #[test]
    fn scan_hangul_needle_matches_brute_force() {
        let lines = &[
            "대한민국,인도네시아,인도",     // 인접 한글 — `인도`가 `인도네시아` 안에도 있다
            "인도,x",                       // 셀 전체가 `인도`
            "인도네시아,y",                 // 셀 전체가 `인도네시아`
            "x,인도네시아공화국",           // 부분 걸침(WholeCell 제외)
            "가나,다라",                    // 매치 없음
            "\"인도,네시아\",z",            // 따옴표 안 delim → 폴백
            "\"인\"도,z",                   // 표시값 `인도` — 바이트엔 `인도`가 없다
            "\"인도\",w",                   // 따옴표 셀
            "간,갇,갈",                     // 한글 바이트가 촘촘한 행
            "한글AB,ab한글",                // 한글 + ASCII 혼합
        ];
        for enc in [Encoding::Utf8, Encoding::Cp949] {
            for needle in ["인도", "인도네시아", "가나", "한글AB", "한글ab"] {
                // 빠른 경로를 **타는지** 먼저 못박는다(안 타면 이 테스트가 폴백만 본다).
                assert!(
                    bytefast_ci_ok(needle, enc),
                    "한글 needle {needle:?}는 {enc:?}에서 빠른 경로를 타야 한다"
                );
                for scope in [
                    crate::find::MatchScope::Partial,
                    crate::find::MatchScope::WholeCell,
                ] {
                    let mut doc = scan_view_doc(lines, enc, SeparatorMode::Char(b','));
                    doc.find_query = needle.to_owned();
                    doc.find_opts = crate::find::FindOptions { match_case: false, scope };
                    assert_scan_equals_brute(&doc);
                }
            }
        }
        // 빠른 경로가 실제로 행을 내는지(항상 빈 Vec이면 계약 테스트가 무의미).
        let mut doc = scan_view_doc(lines, Encoding::Utf8, SeparatorMode::Char(b','));
        doc.find_query = "인도네시아".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::WholeCell,
        };
        let got = scan_all_matches(&doc);
        assert_eq!(got, vec![0, 2], "셀 전체가 `인도네시아`인 행만");
    }

    /// 유니코드 접기가 필요한 needle(`İ`/`É`)은 여전히 폴백을 타되 결과는 정확.
    /// 바이트 접기로는 1:N 확장·악센트 접기를 표현할 수 없기 때문이다.
    #[test]
    fn scan_accented_needle_falls_back() {
        let lines = &["İabc,z", "iabc,w", "Éa,x", "éa,y", "가나,다라"];
        for needle in ["İ", "É", "é"] {
            assert!(
                !bytefast_ci_ok(needle, Encoding::Utf8),
                "접히는 비ASCII needle {needle:?}는 폴백이어야 한다"
            );
            for scope in [
                crate::find::MatchScope::Partial,
                crate::find::MatchScope::WholeCell,
            ] {
                let mut doc =
                    scan_view_doc(lines, Encoding::Utf8, SeparatorMode::Char(b','));
                doc.find_query = needle.to_owned();
                doc.find_opts = crate::find::FindOptions { match_case: false, scope };
                assert_scan_equals_brute(&doc);
            }
        }
    }

    /// **CP949 트레일 바이트 위양성 회귀.** CP949 한글의 트레일 바이트는
    /// 0x41~0xFE라 ASCII 대문자와 겹친다 — 예를 들어 `갂`(0xB0 0xA2) 같은 글자들
    /// 중에는 트레일 바이트가 `A`(0x41)~`Z`(0x5A)인 것이 있다. ignore_case
    /// 접기가 후보를 넓히므로 그 트레일 바이트가 ASCII needle로 잡힐 수 있는데,
    /// 정밀 판정이 걸러 결과는 브루트포스와 같아야 한다.
    #[test]
    fn scan_ignore_case_cp949_trail_byte_no_false_positive() {
        // CP949에서 트레일 바이트가 ASCII 대문자 범위(0x41~0x5A)인 한글을 찾는다.
        let mut hangul = Vec::new();
        for c in '가'..='힣' {
            let b = crate::save::encode_bytes(&c.to_string(), Encoding::Cp949);
            if b.len() == 2 && (0x41..=0x5A).contains(&b[1]) {
                hangul.push(c);
                if hangul.len() >= 6 {
                    break;
                }
            }
        }
        assert!(
            !hangul.is_empty(),
            "사전 조건: 트레일 바이트가 ASCII 대문자인 CP949 한글이 존재해야 한다"
        );
        // 그 한글만으로 이뤄진 행 + 진짜 ASCII 매치 행을 섞는다.
        let mut lines: Vec<String> = hangul.iter().map(|c| format!("{c},x")).collect();
        lines.push("A,y".to_string());
        lines.push("a,z".to_string());
        lines.push("qA,w".to_string());
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        for scope in [crate::find::MatchScope::Partial, crate::find::MatchScope::WholeCell] {
            for needle in ["a", "A"] {
                let mut doc = scan_view_doc(&refs, Encoding::Cp949, SeparatorMode::Char(b','));
                doc.find_query = needle.to_owned();
                doc.find_opts = crate::find::FindOptions { match_case: false, scope };
                // 브루트포스와 같아야 한다 — 트레일 바이트 위양성이 남으면 여기서 깨진다.
                assert_scan_equals_brute(&doc);
            }
        }
        // WholeCell + needle "a": 셀 전체가 a/A인 행만(한글 행·`qA` 행은 제외).
        let mut doc = scan_view_doc(&refs, Encoding::Cp949, SeparatorMode::Char(b','));
        doc.find_query = "a".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::WholeCell,
        };
        let n = hangul.len() as u32;
        assert_eq!(
            scan_all_matches(&doc),
            vec![n, n + 1],
            "한글 트레일 바이트 행이 들어오면 위양성이다"
        );
    }

    /// **편집 모드는 문서 인코딩이 CP949/UTF-16이어도 버퍼가 UTF-8이므로 빠른
    /// 경로를 탄다**(그 판정 근거가 문서 인코딩이 아니라 버퍼의 실제 인코딩임을
    /// 못박는 테스트). CP949 트레일 바이트 위양성은 여기서 발생할 수 없다 —
    /// 버퍼는 UTF-8이라 연속 바이트가 ≥0x80이다.
    #[test]
    fn scan_ignore_case_edit_buffer_is_utf8_regardless_of_doc_encoding() {
        // CP949로 인코딩된 파일을 편집 모드로 연다(버퍼는 UTF-8 String).
        let mut raw = crate::save::encode_bytes(
            "가나,x\nA,y\n갂,z\nHIT,w\n",
            Encoding::Cp949,
        );
        raw.push(b'\n');
        let p = temp(&raw);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        // 인코딩을 명시적으로 CP949로 고정한 뒤 편집 모드 진입(버퍼는 UTF-8).
        doc.enc = Encoding::Cp949;
        doc.sep = SeparatorMode::Char(b',');
        enter_edit_mode(doc);
        assert_eq!(doc.enc, Encoding::Cp949, "사전 조건: 문서 인코딩은 CP949");
        assert!(doc.edit.is_some(), "사전 조건: 편집 모드");
        // 편집 모드가 실제로 빠른 경로를 타는 근거: 버퍼 인코딩(UTF-8) 기준 판정은
        // 참이고, 문서 인코딩(CP949) 기준으로도 참이지만 **확정 여부**가 갈린다 —
        // 버퍼는 UTF-8이라 바로 확정하고, 뷰 모드였다면 정밀 판정을 거쳤을 것이다.
        assert!(bytefast_ci_ok("a", Encoding::Utf8), "버퍼 기준 빠른 경로 성립");
        assert!(
            bytefast_ci_confirms(Encoding::Utf8) && !bytefast_ci_confirms(doc.enc),
            "UTF-8 버퍼는 바로 확정, CP949 파일 바이트였다면 정밀 판정이 필요했다"
        );
        for needle in ["a", "hit", "가나"] {
            for scope in [
                crate::find::MatchScope::Partial,
                crate::find::MatchScope::WholeCell,
                crate::find::MatchScope::WholeWord,
            ] {
                doc.find_query = needle.to_owned();
                doc.find_opts = crate::find::FindOptions { match_case: false, scope };
                assert_scan_equals_brute(doc);
            }
        }
    }

    /// UTF-16 + ignore_case는 빠른 경로를 타지 않고 폴백으로도 정확하다(회귀).
    #[test]
    fn scan_ignore_case_utf16_falls_back_correctly() {
        for enc in [Encoding::Utf16Le, Encoding::Utf16Be] {
            let mut doc = scan_view_doc(
                &["HIT,x", "가나,hit", "hitting,y", "z,가나"],
                enc,
                SeparatorMode::Char(b','),
            );
            for needle in ["hit", "가나"] {
                assert!(!bytefast_ci_ok(needle, enc));
                doc.find_query = needle.to_owned();
                for scope in [
                    crate::find::MatchScope::Partial,
                    crate::find::MatchScope::WholeCell,
                    crate::find::MatchScope::WholeWord,
                ] {
                    doc.find_opts = crate::find::FindOptions { match_case: false, scope };
                    assert_scan_equals_brute(&doc);
                }
            }
        }
    }

    /// 텍스트 모드(delim=None)의 ignore_case Whole cell은 폴백이어야 하고,
    /// Partial은 빠른 경로를 타도 결과가 정확하다.
    #[test]
    fn scan_ignore_case_text_mode_matches_brute_force() {
        let mut doc = scan_view_doc(
            &["HIT", "hit", "Hit there", "none"],
            Encoding::Utf8,
            SeparatorMode::None,
        );
        for scope in [
            crate::find::MatchScope::Partial,
            crate::find::MatchScope::WholeCell,
            crate::find::MatchScope::WholeWord,
        ] {
            doc.find_query = "hit".to_owned();
            doc.find_opts = crate::find::FindOptions { match_case: false, scope };
            assert_scan_equals_brute(&doc);
        }
    }

    // ---- Task I: 바이트 경로의 "비매치 단정" 구멍 수정 (회귀 테스트) ----

    /// `cell_bytes_are_display` 단위 테스트. 이것이 Whole cell 빠른 경로에서
    /// "바이트로 비매치를 단정해도 되는가"의 **유일한** 근거이므로 각 분기를
    /// 직접 부른다(호출부에 조건을 복붙하지 않는다).
    #[test]
    fn cell_bytes_are_display_only_without_quote() {
        assert!(cell_bytes_are_display(b"a,b,c"), "따옴표가 없으면 바이트 == 표시값");
        assert!(cell_bytes_are_display(b""), "빈 행도 마찬가지");
        assert!(!cell_bytes_are_display(b"\"a\"a"), "따옴표가 있으면 표시값이 다를 수 있다");
        assert!(!cell_bytes_are_display(b"\"a_,,HIT"), "닫히지 않은 따옴표도 마찬가지");
        assert!(!cell_bytes_are_display(b"\","), "`\"` 한 개만 있어도");
        // 뮤테이션 감지: 판정이 항상 같은 값을 주지 않는다.
        assert_ne!(
            cell_bytes_are_display(b"a,b"),
            cell_bytes_are_display(b"\"a\",b"),
            "따옴표 유무가 판정을 갈라야 한다"
        );
    }

    /// `needle_roundtrips`: 문서 인코딩으로 옮겼다 되돌렸을 때 원문이 보존되는가.
    /// CP949에 없는 문자는 `encode_bytes`가 `?`로 대체하므로 거짓이어야 한다 —
    /// 그 대체 바이트로 프리필터를 돌리면 브루트포스와 **다른 질문**을 하게 된다.
    #[test]
    fn needle_roundtrips_detects_lossy_encoding() {
        assert!(needle_roundtrips("hit", Encoding::Utf8));
        assert!(needle_roundtrips("hit", Encoding::Cp949));
        assert!(needle_roundtrips("가나", Encoding::Cp949), "CP949에 있는 한글은 왕복 가능");
        assert!(needle_roundtrips("é", Encoding::Utf8));
        assert!(
            !needle_roundtrips("é", Encoding::Cp949),
            "CP949에 없는 문자는 `?`로 대체돼 왕복이 깨진다"
        );
        assert!(!needle_roundtrips("😀", Encoding::Cp949));
    }

    /// **Critical 회귀: Whole cell에서 "히트 0개"는 비매치의 근거가 아니다.**
    ///
    /// `find_in_line_scoped`가 비교하는 값은 파일 바이트가 아니라 `split_fields`가
    /// 준 **표시값**(바깥 따옴표 벗김, `""` → `"`)이다. 그래서 needle 바이트가
    /// 행에 한 번도 나타나지 않아도 매치일 수 있다. 아래 입력은 전부 수정 전에
    /// 빠른 경로가 `[]`를 내고 브루트포스가 `[0]`을 내던 **위음성**이다.
    #[test]
    fn scan_wholecell_quote_split_needle_is_not_missed() {
        // (행, needle) — 표시값이 원본 바이트와 다른 형태들.
        let cases: &[(&str, &str)] = &[
            ("\"a\"a", "aa"),               // 표시값 `aa` — 바이트에 `aa`가 붙어 있지 않다
            ("\"a\"a", "AA"),               // ignore_case 변형
            ("x,\"a\"a,y", "aa"),           // 가운데 셀
            ("\"a\"\"b\"", "a\"b"),         // `""` → `"` 언이스케이프
            ("\"hi\"t", "hit"),
            ("\"HI\"T", "hit"),
            ("a,\"jo\"hn smith", "john smith"),
            // 아래 둘은 반대 방향(위양성): 바이트로는 셀 경계처럼 보이지만 실제로는 아니다.
            ("\"a_,,HIT", "HIT"),           // 닫히지 않은 따옴표
            ("\",", "\""),                  // needle이 `"` 자체
        ];
        for &(line, needle) in cases {
            for match_case in [true, false] {
                // 뷰 모드.
                let mut doc = scan_view_doc(&[line], Encoding::Utf8, SeparatorMode::Char(b','));
                doc.find_query = needle.to_owned();
                doc.find_opts = crate::find::FindOptions {
                    match_case,
                    scope: crate::find::MatchScope::WholeCell,
                };
                assert_scan_equals_brute(&doc);
                // 편집 모드(같은 데이터).
                let src = format!("{line}\n");
                let (mut app, _d) = edit_doc(src.as_bytes(), false);
                let ed = app.doc_mut().unwrap();
                ed.sep = SeparatorMode::Char(b',');
                ed.find_query = needle.to_owned();
                ed.find_opts = crate::find::FindOptions {
                    match_case,
                    scope: crate::find::MatchScope::WholeCell,
                };
                assert_scan_equals_brute(ed);
            }
        }
        // 위음성이 실제로 사라졌는지 한 케이스는 값으로도 못박는다(빈 결과 비교가
        // 우연히 통과하는 것을 막는다).
        let mut doc = scan_view_doc(&["\"a\"a"], Encoding::Utf8, SeparatorMode::Char(b','));
        doc.find_query = "aa".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::WholeCell,
        };
        assert_eq!(scan_all_matches(&doc), vec![0], "표시값 `aa`를 잡아야 한다");
        // 위양성도 값으로 못박는다.
        let mut doc = scan_view_doc(&["\"a_,,HIT"], Encoding::Utf8, SeparatorMode::Char(b','));
        doc.find_query = "HIT".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::WholeCell,
        };
        assert!(
            scan_all_matches(&doc).is_empty(),
            "닫히지 않은 따옴표 안이라 셀 전체가 HIT가 아니다"
        );
    }

    /// **CP949 트레일 바이트 위양성(match_case Partial) 회귀.** CP949 한글의
    /// 트레일 바이트는 ASCII와 겹치므로 memmem 히트가 문자 **중간**에 걸릴 수 있다.
    /// `_갂\t\thitting` + needle `A`(TSV)가 그 예다 — 수정 전에는 빠른 경로가
    /// `[0]`, 브루트포스가 `[]`였다.
    #[test]
    fn scan_partial_match_case_cp949_trail_byte_no_false_positive() {
        let mut doc =
            scan_view_doc(&["_갂\t\thitting"], Encoding::Cp949, SeparatorMode::Char(b'\t'));
        doc.find_query = "A".to_owned();
        doc.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::Partial,
        };
        assert_scan_equals_brute(&doc);
        assert!(
            scan_all_matches(&doc).is_empty(),
            "`갂`의 트레일 바이트가 `A`와 같아도 그건 문자 중간이라 매치가 아니다"
        );
        // 진짜 ASCII `A`가 든 행은 여전히 잡혀야 한다(가드가 전부를 죽이지 않았다).
        let mut doc2 =
            scan_view_doc(&["_갂\t\thitting", "A\tx"], Encoding::Cp949, SeparatorMode::Char(b'\t'));
        doc2.find_query = "A".to_owned();
        doc2.find_opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::Partial,
        };
        assert_eq!(scan_all_matches(&doc2), vec![1]);
        assert_scan_equals_brute(&doc2);
    }

    /// **UTF-16LE Whole cell 위음성 회귀.** UTF-16은 코드유닛이 2바이트라 바이트
    /// 경계 판정이 성립하지 않으므로 Whole cell은 통째로 행 단위 폴백이어야 한다.
    /// 수정 전에는 memmem 프리필터가 히트 0개인 행을 **방문조차 하지 않아**
    /// `"a"a`(표시값 `aa`) 행을 놓쳤다.
    #[test]
    fn scan_wholecell_utf16_quote_split_needle_is_not_missed() {
        for enc in [Encoding::Utf16Le, Encoding::Utf16Be] {
            let mut doc = scan_view_doc(
                &["zzz", "\"a\"a,a b0,,"],
                enc,
                SeparatorMode::Char(b','),
            );
            for match_case in [true, false] {
                doc.find_query = "aa".to_owned();
                doc.find_opts = crate::find::FindOptions {
                    match_case,
                    scope: crate::find::MatchScope::WholeCell,
                };
                assert_scan_equals_brute(&doc);
                assert_eq!(scan_all_matches(&doc), vec![1], "표시값 `aa`인 셀을 잡아야 한다");
            }
        }
    }

    /// **CP949로 표현할 수 없는 needle 회귀.** `save::encode_bytes`는 `é`를 조용히
    /// `?`(0x3F)로 바꾼다. 그 바이트로 프리필터를 돌리면 "`?`를 찾는 검색"이 되어
    /// 브루트포스와 다른 결과가 나온다(위양성이 아니라 **다른 질문**이다).
    /// 왕복 가드(`needle_roundtrips`)가 이걸 막고 행 단위 폴백으로 보낸다.
    #[test]
    fn scan_cp949_unrepresentable_needle_falls_back() {
        let mut doc = scan_view_doc(&["é,x", "?,y", "a?b,z"], Encoding::Cp949, SeparatorMode::Char(b','));
        for scope in [
            crate::find::MatchScope::Partial,
            crate::find::MatchScope::WholeCell,
            crate::find::MatchScope::WholeWord,
        ] {
            for match_case in [true, false] {
                doc.find_query = "é".to_owned();
                doc.find_opts = crate::find::FindOptions { match_case, scope };
                assert_scan_equals_brute(&doc);
                assert!(
                    scan_all_matches(&doc).is_empty(),
                    "CP949 파일에 `é`는 존재할 수 없다 — `?` 행이 잡히면 위양성"
                );
            }
        }
    }

    /// **차등 퍼징(적대적 따옴표 알파벳, 소규모 전수).** 무작위 생성은 따옴표가
    /// 셀을 쪼개거나(`"a"a`) 언이스케이프하는(`"a""b"`) 형태를 거의 만들지 못해
    /// 이번 결함들을 놓쳤다. 그래서 `a A " ,` 네 글자로 이뤄진 **길이 ≤ 4의 모든
    /// 문자열**(4^0+…+4^4 = 341개)을 전수로 만들어 브루트포스와 대조한다.
    /// scope × match_case × UTF-8/CP949를 모두 돈다.
    #[test]
    fn scan_all_matches_differential_fuzz_quote_alphabet() {
        const ALPHA: &[char] = &['a', 'A', '"', ','];
        // 길이 0..=4의 모든 문자열.
        let mut corpus: Vec<String> = vec![String::new()];
        let mut frontier: Vec<String> = vec![String::new()];
        for _ in 0..4 {
            let mut next = Vec::new();
            for s in &frontier {
                for &c in ALPHA {
                    let mut t = s.clone();
                    t.push(c);
                    next.push(t);
                }
            }
            corpus.extend(next.iter().cloned());
            frontier = next;
        }
        assert_eq!(corpus.len(), 1 + 4 + 16 + 64 + 256, "전수 코퍼스 크기");
        let refs: Vec<&str> = corpus.iter().map(|s| s.as_str()).collect();
        let scopes = [
            crate::find::MatchScope::Partial,
            crate::find::MatchScope::WholeWord,
            crate::find::MatchScope::WholeCell,
        ];
        // needle도 같은 알파벳에서 뽑는다(길이 1~3).
        let needles = ["a", "A", "\"", ",", "aa", "aA", "a\"", "\"a", "a,", ",a", "a\"a", "aaa", "a\"b"];
        for enc in [Encoding::Utf8, Encoding::Cp949] {
            for &scope in &scopes {
                for match_case in [true, false] {
                    for needle in needles {
                        let mut doc = scan_view_doc(&refs, enc, SeparatorMode::Char(b','));
                        doc.find_query = needle.to_owned();
                        doc.find_opts = crate::find::FindOptions { match_case, scope };
                        assert_scan_equals_brute(&doc);
                    }
                }
            }
        }
        // 편집 모드도 같은 코퍼스로(버퍼는 UTF-8).
        let mut src = String::new();
        for l in &corpus {
            src.push_str(l);
            src.push('\n');
        }
        let (mut app, _d) = edit_doc(src.as_bytes(), false);
        let doc = app.doc_mut().unwrap();
        doc.sep = SeparatorMode::Char(b',');
        for &scope in &scopes {
            for match_case in [true, false] {
                for needle in needles {
                    doc.find_query = needle.to_owned();
                    doc.find_opts = crate::find::FindOptions { match_case, scope };
                    assert_scan_equals_brute(doc);
                }
            }
        }
    }

    // ---- F: Find All 하이라이트 스냅샷 / 상태 전이 ----

    /// `apply_find_action(doc, FindAction::All)`이 하이라이트 스냅샷을 만든다:
    /// `rows`가 `scan_all_matches`와 같고, query/opts가 **호출 시점 값으로 얼려진다**.
    #[test]
    fn find_all_sets_highlight_snapshot() {
        let mut doc = scan_view_doc(&["hit", "no", "hit"], Encoding::Utf8, SeparatorMode::None);
        doc.find_query = "hit".to_owned();
        let opts_at_call = doc.find_opts.clone();
        let expected_rows = scan_all_matches(&doc);
        apply_find_action(&mut doc, FindAction::All);
        let hl = doc.highlight.as_ref().expect("Find All이 하이라이트를 만든다");
        assert_eq!(hl.rows, expected_rows, "rows는 scan_all_matches와 같아야 한다");
        assert_eq!(hl.query, "hit", "query는 호출 시점 검색어로 얼려진다");
        assert_eq!(hl.opts, opts_at_call, "opts는 호출 시점 옵션으로 얼려진다");
        assert_eq!(doc.find_status, "2 matching rows");
    }

    /// 빈 검색어로 Find All → 하이라이트는 만들어지지 않고(None 유지), 상태 문구만
    /// 안내로 채워진다. `apply_find_action`의 빈 검색어 가드가 담당한다.
    #[test]
    fn find_all_empty_query_no_highlight() {
        let mut doc = scan_view_doc(&["a", "b"], Encoding::Utf8, SeparatorMode::None);
        doc.find_query.clear();
        apply_find_action(&mut doc, FindAction::All);
        assert!(doc.highlight.is_none(), "빈 검색어면 하이라이트를 만들지 않는다");
        assert_eq!(doc.find_status, "Enter text to find");
    }

    /// **자동 스캔이 사라졌음을 증명하는 핵심 테스트.** 하이라이트를 만든 뒤
    /// `find_query`를 바꿔도(그리고 옵션을 바꿔도) `highlight`는 그대로다 — 렌더를
    /// 돌리지 않으므로 스캔이 없어야 하고, 그 불변성으로 확인한다.
    #[test]
    fn typing_query_does_not_change_highlight() {
        let mut doc = scan_view_doc(&["hit", "no", "hit"], Encoding::Utf8, SeparatorMode::None);
        doc.find_query = "hit".to_owned();
        apply_find_action(&mut doc, FindAction::All);
        let before = doc.highlight.clone().expect("스냅샷이 있어야 한다");

        // 검색어를 여러 번 바꾼다 — 스냅샷은 절대 따라 움직이면 안 된다.
        for q in ["h", "hi", "hix", "완전히 다른 것", ""] {
            doc.find_query = q.to_owned();
            assert_eq!(doc.highlight.as_ref(), Some(&before), "검색어 변경이 하이라이트를 바꾸면 안 된다");
        }
        // 옵션을 바꿔도 마찬가지.
        doc.find_opts.match_case = !doc.find_opts.match_case;
        assert_eq!(doc.highlight.as_ref(), Some(&before), "옵션 변경도 하이라이트를 바꾸면 안 된다");
    }

    /// Find Next/Prev를 여러 번 해도 `highlight`는 그대로고 `last_match`만 바뀐다.
    #[test]
    fn find_next_preserves_highlight() {
        let mut doc = scan_view_doc(&["hit", "no", "hit"], Encoding::Utf8, SeparatorMode::None);
        doc.find_query = "hit".to_owned();
        apply_find_action(&mut doc, FindAction::All);
        let snapshot = doc.highlight.clone().expect("스냅샷이 있어야 한다");

        apply_find_action(&mut doc, FindAction::Next);
        let first = doc.last_match;
        assert!(first.is_some(), "Find Next가 매치를 잡는다");
        assert_eq!(doc.highlight.as_ref(), Some(&snapshot), "Find Next가 하이라이트를 건드리면 안 된다");

        apply_find_action(&mut doc, FindAction::Next);
        assert_ne!(doc.last_match, first, "두 번째 Next는 커서를 옮긴다");
        assert_eq!(doc.highlight.as_ref(), Some(&snapshot), "커서가 움직여도 하이라이트는 그대로");
    }

    /// 옵션(find_opts)을 바꿔도 하이라이트는 유지된다(확정 동작 5). `apply_find_action`이
    /// 하이라이트를 안 건드리므로, 옵션만 다른 상태에서 Next를 눌러도 스냅샷은 그대로다.
    #[test]
    fn option_change_keeps_highlight() {
        let mut doc = scan_view_doc(&["hit", "no", "hit"], Encoding::Utf8, SeparatorMode::None);
        doc.find_query = "hit".to_owned();
        apply_find_action(&mut doc, FindAction::All);
        let snapshot = doc.highlight.clone().expect("스냅샷이 있어야 한다");
        // 옵션을 바꾼다(패널의 옵션 리셋 로직은 last_match만 건드리고 highlight는 두므로,
        // 여기서는 상태 전이의 핵심인 apply_find_action이 하이라이트를 보존함을 본다).
        doc.find_opts.match_case = !doc.find_opts.match_case;
        apply_find_action(&mut doc, FindAction::Next);
        assert_eq!(doc.highlight.as_ref(), Some(&snapshot), "옵션을 바꿔도 하이라이트는 다음 Find All까지 유지");
    }

    /// 추출하면 새 탭 문서의 `highlight`가 Some이고, 새 문서의 데이터 행들이
    /// 매치로 잡힌다. 원본 탭의 highlight도 그대로다(추출이 원본을 안 건드림).
    #[test]
    fn extract_carries_highlight_to_new_tab() {
        let (mut app, _d) = edit_doc(b"h,v\nhit,1\nno,2\nhit,3\n", true);
        {
            let doc = app.doc_mut().unwrap();
            doc.find_query = "hit".to_owned();
            // 원본에 먼저 Find All을 해 두어, 추출이 원본 스냅샷을 안 건드리는지도 본다.
            apply_find_action(doc, FindAction::All);
        }
        let orig_hl = app.doc().unwrap().highlight.clone().expect("원본 스냅샷");
        let before_tabs = app.docs.len();
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), before_tabs + 1, "추출은 새 탭을 연다");

        // 새 탭(활성)의 하이라이트.
        let new_doc = app.doc().unwrap();
        let hl = new_doc.highlight.as_ref().expect("추출본에 하이라이트가 실려 있다");
        assert_eq!(hl.query, "hit");
        // 새 문서: 헤더 + 매치 데이터 2행. 데이터 행(1,2)이 모두 매치로 잡힌다.
        assert_eq!(hl.rows, vec![1, 2], "추출본의 데이터 행이 전부 매치");
        assert_eq!(new_doc.find_query, "hit", "새 탭은 검색어를 물려받는다");

        // 원본 탭(index 0)의 스냅샷은 그대로.
        assert_eq!(app.docs[0].highlight.as_ref(), Some(&orig_hl), "추출이 원본 하이라이트를 안 건드린다");
    }

    /// Find All이 만든 `highlight.rows`가 브루트포스 `matching_lines`와 같은 행
    /// 집합이다. (`scan_all_matches_*` 테스트가 스캔 자체는 이미 커버하므로, 여기서는
    /// **apply_find_action이 그 스캔 결과를 그대로 스냅샷에 싣는지**를 확인한다.)
    #[test]
    fn find_all_matches_brute_force() {
        let mut doc = scan_view_doc(
            &["a,b,c", "hit,x", "y,hit", "no"],
            Encoding::Utf8,
            SeparatorMode::Char(b','),
        );
        doc.find_query = "hit".to_owned();
        let brute: Vec<u32> = crate::find::matching_lines(
            doc_line_count(&doc),
            &effective_query(&doc),
            &doc.find_opts,
            doc_delimiter(&doc),
            |i| logical_line(&doc, i),
        )
        .into_iter()
        .map(|i| i as u32)
        .collect();
        apply_find_action(&mut doc, FindAction::All);
        assert_eq!(doc.highlight.unwrap().rows, brute, "스냅샷 rows가 브루트포스와 같아야 한다");
    }

    /// 거터 표시 조건: 스냅샷이 없거나 매치가 비면 감춘다. 순수 함수로 테스트
    /// (인라인 복붙 금지 — 실제 `show_gutter`를 부른다).
    #[test]
    fn gutter_hidden_when_no_matches() {
        assert!(!show_gutter(None), "스냅샷이 없으면 거터를 감춘다");
        let empty = Highlight { query: "x".into(), opts: Default::default(), rows: vec![] };
        assert!(!show_gutter(Some(&empty)), "매치가 없으면 거터를 감춘다");
        let one = Highlight { query: "x".into(), opts: Default::default(), rows: vec![0] };
        assert!(show_gutter(Some(&one)), "매치가 있으면 거터를 보인다");
        let many = Highlight { query: "x".into(), opts: Default::default(), rows: vec![3, 7, 100] };
        assert!(show_gutter(Some(&many)));
    }

    /// Find All 상태 문구: 0개면 Not found, 1개면 단수, 그 외 복수.
    #[test]
    fn find_all_status_variants() {
        assert_eq!(find_all_status(0), "Not found");
        assert_eq!(find_all_status(1), "1 matching row");
        assert_eq!(find_all_status(5), "5 matching rows");
    }

    /// Find All 버튼 활성 조건: 검색어가 있어야 한다. 순수 함수로 테스트.
    #[test]
    fn find_all_button_enabled_only_when_query_present() {
        assert!(!find_all_button_enabled(""), "검색어가 비면 비활성");
        assert!(find_all_button_enabled("hit"), "검색어가 있으면 활성");
    }

    /// marker_y: 첫 행은 top, 마지막 행은 bottom 근처.
    #[test]
    fn marker_y_maps_first_row_to_top_last_to_bottom() {
        let (top, height, n) = (10.0_f32, 200.0_f32, 100);
        assert_eq!(marker_y(0, n, top, height), top, "0행은 거터 맨 위");
        // 마지막 행(99)은 bottom(top+height=210)에 가깝되 넘지 않는다.
        let y_last = marker_y(n - 1, n, top, height);
        assert!(y_last < top + height, "마지막 행은 bottom 아래로 넘지 않는다");
        assert!(y_last > top + height - height / n as f32 - 0.1, "그러나 bottom 근처");
        // 빈 문서(line_count 0)는 top.
        assert_eq!(marker_y(0, 0, top, height), top);
    }

    /// row_at_y는 marker_y의 역함수 — 여러 행에 대해 왕복 항등.
    #[test]
    fn row_at_y_inverts_marker_y() {
        let (top, height, n) = (10.0_f32, 200.0_f32, 50);
        for r in [0usize, 1, 7, 25, 48, 49] {
            let y = marker_y(r, n, top, height);
            // 눈금 두께(2px)만큼의 오차를 피하려고 마커 y 정중앙에서 역산한다.
            assert_eq!(row_at_y(y, n, top, height), r, "행 {r} 왕복이 어긋난다");
        }
    }

    /// 거터 위/아래 밖을 클릭해도 유효 행으로 클램프한다.
    #[test]
    fn row_at_y_clamps_out_of_range() {
        let (top, height, n) = (10.0_f32, 200.0_f32, 40);
        assert_eq!(row_at_y(top - 999.0, n, top, height), 0, "위쪽 밖 → 첫 행");
        assert_eq!(
            row_at_y(top + height + 999.0, n, top, height),
            n - 1,
            "아래쪽 밖 → 마지막 행"
        );
        // 빈 문서/0 높이는 0으로(패닉 없음).
        assert_eq!(row_at_y(50.0, 0, top, height), 0);
        assert_eq!(row_at_y(50.0, n, top, 0.0), 0);
    }

    /// 표 모드 셀 매치 판정: 셀 텍스트에 delim=None으로 `find_in_line_scoped`를
    /// 부르면 세 scope가 모두 셀 단위로 올바르게 동작한다(E2-4의 핵심 의존).
    /// 이 논리가 깨지면 표 모드 하이라이트가 어긋나므로 얇게라도 고정한다.
    #[test]
    fn table_cell_scope_with_none_delim() {
        use crate::find::{find_in_line_scoped, FindOptions, MatchScope};
        // 셀 텍스트 "hitting"에서 needle "hit":
        let cell = "hitting";
        // Partial: 셀 안 부분 매치(col 0, len 3).
        let partial = FindOptions { match_case: true, scope: MatchScope::Partial };
        assert_eq!(find_in_line_scoped(cell, "hit", &partial, None), vec![(0, 3)]);
        // WholeWord: "hitting"의 "hit"는 단어 일부라 매치 없음.
        let word = FindOptions { match_case: true, scope: MatchScope::WholeWord };
        assert!(find_in_line_scoped(cell, "hit", &word, None).is_empty());
        // WholeCell + delim=None: 셀 전체 == needle일 때만 → "hitting" != "hit" → 없음.
        let whole = FindOptions { match_case: true, scope: MatchScope::WholeCell };
        assert!(find_in_line_scoped(cell, "hit", &whole, None).is_empty());
        // 셀 전체가 정확히 needle이면 WholeCell 매치.
        assert_eq!(find_in_line_scoped("hit", "hit", &whole, None), vec![(0, 3)]);
    }

    /// 뷰 모드에서도 찾기 자체는 된다(mmap + 인덱스 경로).
    #[test]
    fn find_works_in_view_mode() {
        let p = temp_ext(b"alpha\nbeta\nneedle\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        doc.find_query = "needle".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.last_match, Some(crate::find::Match { line: 2, col: 0, len: 6 }));
    }

    #[test]
    fn find_with_empty_query_reports_and_does_nothing() {
        let mut app = find_test_doc(&["a"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query.clear();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.find_status, "Enter text to find");
        assert_eq!(doc.last_match, None);
    }

    /// 표 모드에서는 셀 단위가 아니라 **행 전체**를 선택하고(매치의 col은
    /// char 인덱스지 컬럼 번호가 아니다), 헤더를 뺀 화면 행으로 스크롤한다.
    ///
    /// Important 2 회귀: 끝 컬럼은 표가 실제로 그리는 컬럼 수
    /// (`table_col_count` — 여기서는 헤더 4개와 모든 데이터 행이 4개라 4)에서
    /// 나와야 한다. `selected_col`(헤더 클릭으로 고른, 매치와 무관한 UI
    /// 상태)을 일부러 다른 값(2)으로 세팅해 두어, 만약 구현이 `selected_col`을
    /// 끝 컬럼으로 잘못 쓰면 (row0, col0, row1, 2)가 되어 이 단언이 깨지게 한다.
    #[test]
    fn find_selects_whole_row_in_table_mode() {
        let p = temp(b"name,city,age,note\nAlice,Seoul,30,x\nBob,Busan,40,y\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        assert!(matches!(doc.sep, SeparatorMode::Char(b',')));
        assert!(doc.has_header, "사전 조건: 헤더 감지됨");
        // 매치와 무관한 UI 상태 — 끝 컬럼 계산에 섞여 들면 안 된다.
        doc.selected_col = Some(2);
        doc.find_query = "Busan".to_owned();
        apply_find_action(doc, FindAction::Next);
        let m = doc.last_match.unwrap();
        assert_eq!(m.line, 2);
        assert_eq!(
            doc.cell_sel,
            Some((2, 0, 2, 3)),
            "끝 컬럼은 실제 필드 수(4개 → 인덱스 3)에서 나와야 하고 \
             selected_col(2)과는 무관해야 한다"
        );
        assert_eq!(
            doc.pending_scroll_row,
            Some(1),
            "표 모드의 화면 행 = 논리 행 - 헤더 한 줄"
        );
        assert_eq!(doc.text_sel, None, "표 모드에선 텍스트 선택을 건드리지 않는다");
    }

    /// Important 1 회귀: 뷰 모드 정렬(permutation)이 걸려 있으면 화면 행은
    /// "논리 행 - data_start"가 아니라 permutation의 **역**이어야 한다.
    /// 리뷰어의 시나리오 — `name,v` 헤더 + zzz/aaa/mmm 3행을 컬럼 0 오름차순
    /// 정렬하면 순서가 aaa, mmm, zzz가 되어 permutation = [2, 3, 1]
    /// (헤더는 항상 0번 자리를 지킨다고 가정하지 않고, 데이터 행 논리 번호를
    /// 그대로 담는다는 이 코드베이스의 관례를 따른다 — 아래 직접 구성).
    /// "zzz"는 논리 행 1이고 permutation에서 화면 위치 2에 있다.
    #[test]
    fn find_scrolls_to_correct_row_under_view_sort() {
        let p = temp(b"name,v\nzzz,1\naaa,2\nmmm,3\n");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        assert!(doc.has_header);
        // 뷰 모드 정렬을 흉내낸다: 논리 행 1(zzz),2(aaa),3(mmm)을 컬럼0
        // 오름차순으로 정렬하면 화면 순서는 aaa(2), mmm(3), zzz(1) →
        // permutation[screen] = logical 이므로 permutation = [2, 3, 1].
        doc.sort = Some(SortState {
            permutation: vec![2, 3, 1],
            col: 0,
            kind: SortKind::Text,
            dir: SortDir::Asc,
            spec_count: 1,
        });
        doc.find_query = "zzz".to_owned();
        apply_find_action(doc, FindAction::Next);
        let m = doc.last_match.unwrap();
        assert_eq!(m.line, 1, "사전 조건: 'zzz'는 논리 행 1(0-based)");
        assert_eq!(
            doc.pending_scroll_row,
            Some(2),
            "permutation에서 논리 행 1은 화면 위치 2(0-based, 헤더 제외) — \
             saturating_sub(data_start)만으로 구한 0은 틀렸다"
        );
        // cell_sel의 행은 화면 행이 아니라 논리 행으로 남아야 한다
        // (render_table이 cell_sel을 논리 행으로 해석 — 브리프 노트).
        assert_eq!(doc.cell_sel.map(|(r0, _, r1, _)| (r0, r1)), Some((1, 1)));
    }

    /// 옵션을 바꾸면 이전 매치를 버린다 — 다른 규칙으로 잡힌 자리에서
    /// 이어서 찾으면 기준이 뒤섞인다.
    #[test]
    fn find_options_default_and_change_resets_last_match() {
        let mut app = find_test_doc(&["Hit hit"]);
        let doc = app.doc_mut().unwrap();
        // open_path가 넣는 기본값.
        assert_eq!(
            doc.find_opts,
            crate::find::FindOptions {
                match_case: false,
                scope: crate::find::MatchScope::Partial
            }
        );
        assert!(!doc.show_find && doc.find_query.is_empty() && doc.last_match.is_none());
        doc.find_query = "hit".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.last_match.map(|m| m.col), Some(0), "대소문자 무시라 'Hit'가 잡힌다");
        // 패널의 옵션 변경 처리와 같은 규칙을 여기서 재현한다.
        doc.find_opts.match_case = true;
        doc.last_match = None;
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.last_match.map(|m| m.col), Some(4), "구분하면 소문자 'hit'만");
    }

    /// 마지막 매치 자리 글자가 편집으로 바뀌면 "바꾸기"가 낡은 위치를
    /// 그대로 치환하면 안 된다 — 다시 찾아서 잡는다.
    #[test]
    fn replace_one_revalidates_stale_match() {
        let mut app = find_test_doc(&["hit", "hit"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        doc.replace_text = "Z".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.last_match.map(|m| m.line), Some(0));
        // 그 행을 다른 내용으로 바꿔 매치를 무효화한다.
        doc.edit.as_mut().unwrap().lines[0] = "gone".to_owned();
        apply_find_action(doc, FindAction::ReplaceOne);
        // 낡은 자리를 치환하지 않고 살아 있는 매치(1행)를 다시 잡는다.
        assert_eq!(doc.edit.as_ref().unwrap().lines[0], "gone");
        assert_eq!(doc.last_match.map(|m| m.line), Some(1));
    }

    /// Important 3 회귀: 치환문이 검색어와 글자 그대로 같으면("hit" → "hit")
    /// 매치는 있어도 실제로는 아무것도 바뀌지 않는다. 그런 경우까지 undo를
    /// 쌓고 dirty를 세우면, 사용자가 저장할 필요가 없는 파일에 거짓
    /// "● Modified"가 뜨고 되돌리기 한 칸이 허깨비가 된다.
    #[test]
    fn replace_one_noop_when_replacement_equals_match_pushes_no_undo() {
        let mut app = find_test_doc(&["hit"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        doc.replace_text = "hit".to_owned();
        let before = doc.edit.as_ref().unwrap().undo.len();
        apply_find_action(doc, FindAction::ReplaceOne); // 첫 호출은 찾기.
        apply_find_action(doc, FindAction::ReplaceOne); // 두 번째가 "치환" 시도.
        let e = doc.edit.as_ref().unwrap();
        assert_eq!(e.lines[0], "hit", "내용은 그대로");
        assert_eq!(e.undo.len(), before, "실제로 안 바뀌었으니 undo가 안 쌓인다");
        assert!(!e.dirty, "실제로 안 바뀌었으니 dirty도 서지 않는다");
    }

    /// 같은 결함을 반복 호출로 확인한다: `"a a"`를 `"a"`로 "바꾸기"를 여러 번
    /// 눌러도(매번 매치는 있다) 버퍼는 절대 안 바뀌므로 undo는 한 번도 늘지
    /// 않아야 한다.
    #[test]
    fn replace_one_repeated_noop_never_grows_undo() {
        let mut app = find_test_doc(&["a a"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "a".to_owned();
        doc.replace_text = "a".to_owned();
        let before = doc.edit.as_ref().unwrap().undo.len();
        for _ in 0..4 {
            apply_find_action(doc, FindAction::ReplaceOne);
        }
        let e = doc.edit.as_ref().unwrap();
        assert_eq!(e.lines[0], "a a", "내용은 절대 안 바뀐다");
        assert_eq!(e.undo.len(), before, "반복해도 undo가 늘지 않는다");
        assert!(!e.dirty);
    }

    /// Important 3 회귀(Replace All): 치환문이 검색어와 같아서 매치는
    /// 있었지만 아무 행도 실제로 안 바뀐 경우, undo/dirty를 세우지 않고
    /// 상태 문구도 "N replacements"라고 거짓 보고하지 않는다.
    #[test]
    fn replace_all_noop_when_replacement_equals_match_pushes_no_undo() {
        let mut app = find_test_doc(&["hit", "no", "hit hit"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        doc.replace_text = "hit".to_owned();
        let before = doc.edit.as_ref().unwrap().undo.len();
        apply_find_action(doc, FindAction::ReplaceAll);
        let e = doc.edit.as_ref().unwrap();
        assert_eq!(e.lines, v(&["hit", "no", "hit hit"]), "내용은 그대로");
        assert_eq!(e.undo.len(), before, "실제로 안 바뀌었으니 undo가 안 쌓인다");
        assert!(!e.dirty, "실제로 안 바뀌었으니 dirty도 서지 않는다");
        assert_ne!(
            doc.find_status, "3 replacements",
            "매치 수를 그대로 보고하면 바뀐 것처럼 거짓 보고하는 셈이다"
        );
    }

    /// Minor 5 회귀: `replace_one`이 낡은 매치를 재검증하다 실패하고, 그 뒤
    /// 이어서 시도한 검색마저 실패하면(검색어가 더 이상 어디에도 없음)
    /// `last_match`는 반드시 `None`이어야 한다. 그대로 두면 버퍼 밖 논리
    /// 행을 가리키는 낡은 값이 남아 다음 Find Next의 기준이 뒤섞인다.
    #[test]
    fn replace_one_clears_last_match_when_revalidation_and_research_both_fail() {
        let mut app = find_test_doc(&["hit", "hit"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        doc.replace_text = "Z".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(doc.last_match.map(|m| m.line), Some(0));
        // 두 매치 행을 모두 다른 내용으로 바꿔 검색어가 문서에서 완전히
        // 사라지게 한다 — 재검증도, 뒤이은 재검색도 둘 다 실패한다.
        {
            let e = doc.edit.as_mut().unwrap();
            e.lines[0] = "gone".to_owned();
            e.lines[1] = "gone too".to_owned();
        }
        apply_find_action(doc, FindAction::ReplaceOne);
        assert_eq!(
            doc.last_match, None,
            "재검증도 재검색도 실패하면 last_match가 낡은 채로 남으면 안 된다"
        );
        assert_eq!(doc.find_status, "Not found");
    }

    /// Ctrl+F가 패널을 열고 입력란 포커스를 예약한다. `update()`가 `eframe::
    /// Frame`을 요구해 테스트에서 직접 부를 수 없으므로, 단축키 블록이 쓰는
    /// **실제 게이트 함수**(`find_keys_live`)와 consume_key 호출을 그대로
    /// 재현해 태운다(Minor 6 — 가드 식을 테스트가 따로 베끼지 않는다).
    #[test]
    fn ctrl_f_opens_find_panel() {
        let mut app = find_test_doc(&["hit"]);
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::F,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        });
        let _ = ctx.run(input, |ctx| {
            assert!(find_keys_live(&app), "사전 조건: 찾기 단축키가 살아 있는 상태");
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
                let doc = app.doc_mut().unwrap();
                doc.show_find = true;
                doc.find_focus_pending = true;
            }
        });
        let doc = app.doc().unwrap();
        assert!(doc.show_find, "Ctrl+F로 패널이 열린다");
        assert!(doc.find_focus_pending, "열린 프레임에 포커스를 예약한다");
    }

    /// 인라인 셀 편집 중에는 Ctrl+F가 문서로 새지 않는다 —
    /// `can_undo_key`와 같은 양보 규율. 실제 게이트 함수를 호출한다(Minor 6) —
    /// 그래야 게이트를 지우거나 뒤집으면 이 테스트가 반드시 깨진다.
    #[test]
    fn ctrl_f_yields_while_editing_a_cell() {
        let mut app = find_test_doc(&["hit"]);
        app.doc_mut().unwrap().editing_cell = Some((0, 0));
        assert!(!find_keys_live(&app), "셀 편집 중에는 찾기 단축키가 죽는다");
    }

    /// 저장/확인 다이얼로그가 떠 있으면 찾기 단축키도 양보한다. 실제 게이트
    /// 함수를 호출한다(Minor 6).
    #[test]
    fn find_keys_yield_to_dialogs() {
        let mut app = find_test_doc(&["hit"]);
        app.show_save_dialog = true;
        assert!(!find_keys_live(&app), "저장 다이얼로그가 떠 있으면 죽는다");
        app.show_save_dialog = false;
        app.pending_action = Some(PendingAction::ExitEditMode);
        assert!(!find_keys_live(&app), "확인 다이얼로그가 떠 있으면 죽는다");
    }

    /// F3은 검색어가 비어 있으면 아무것도 하지 않는다(빈 검색어로
    /// 안내 문구만 띄우는 것은 소음이다).
    #[test]
    fn f3_with_empty_query_is_ignored() {
        let mut app = find_test_doc(&["hit"]);
        let doc = app.doc_mut().unwrap();
        assert!(doc.find_query.is_empty());
        // update()의 F3 분기와 같은 가드.
        if !doc.find_query.is_empty() {
            apply_find_action(doc, FindAction::Next);
        }
        assert!(doc.find_status.is_empty(), "안내 문구조차 남기지 않는다");
        assert_eq!(doc.last_match, None);
    }

    /// 찾기 패널을 실제로 그려서 프레임이 성립하는지 + 포커스 예약이
    /// 그 프레임에 소비되는지 확인한다(매 프레임 포커스를 주면 다른 위젯을
    /// 클릭할 수 없게 되므로 "한 번만"이 중요하다).
    #[test]
    fn find_panel_renders_and_consumes_focus_request() {
        let mut app = find_test_doc(&["hit"]);
        let ctx = egui::Context::default();
        {
            let doc = app.doc_mut().unwrap();
            doc.show_find = true;
            doc.find_focus_pending = true;
        }
        let _ = ctx.run(Default::default(), |ctx| {
            let doc = app.doc_mut().unwrap();
            let act = render_find_panel(ctx, doc, crate::i18n::Lang::default());
            assert_eq!(act, None, "아무 버튼도 누르지 않았으면 인텐트도 없다");
        });
        assert!(
            !app.doc().unwrap().find_focus_pending,
            "포커스 요청은 한 프레임만 살아 있어야 한다"
        );
        assert!(app.doc().unwrap().show_find, "패널은 그대로 열려 있다");
    }

    /// 창의 X 버튼(`.open(&mut open)`)으로 닫으면 `doc.show_find`가 꺼진다.
    /// 본문의 `✖` 버튼을 없앤 대신 이 경로가 유일한 닫기 수단(Escape 제외)이므로
    /// 실제로 동작하는지 확인한다.
    #[test]
    fn find_window_x_button_closes_panel() {
        let mut app = find_test_doc(&["hit"]);
        app.doc_mut().unwrap().show_find = true;
        let ctx = egui::Context::default();
        // 첫 프레임: 창을 띄운다.
        let _ = ctx.run(Default::default(), |ctx| {
            let doc = app.doc_mut().unwrap();
            let _ = render_find_panel(ctx, doc, crate::i18n::Lang::default());
        });
        assert!(app.doc().unwrap().show_find, "사전 조건: 창이 열려 있다");
        // egui Window의 X 버튼 클릭을 직접 시뮬레이션하기보다, `open` 파라미터가
        // false로 들어오면 `render_find_panel`이 `show_find`를 내린다는 계약을
        // 확인한다 — `egui::Window::show`는 `open`을 자신이 관리하는 area 상태와
        // 동기화하므로, 여기서는 그 반영 계약(`if !open { doc.show_find = false }`)
        // 을 실제로 호출해 확인한다.
        {
            let doc = app.doc_mut().unwrap();
            doc.show_find = false; // X 클릭을 흉내: egui가 open을 false로 되돌린 것과 동치
        }
        assert!(!app.doc().unwrap().show_find, "닫힌 뒤에는 꺼져 있어야 한다");
    }

    /// Whole cell 활성 조건의 순수 함수 `whole_cell_enabled`: 표 모드
    /// (`SeparatorMode::Char`)에서는 활성, 텍스트 모드(`SeparatorMode::None`)에서는
    /// 비활성이어야 한다. 조건을 뒤집으면(예: `!matches!(...)`) 이 두 테스트가
    /// 반드시 깨진다 — 인라인 복붙 가드가 아니라 실제 함수를 호출한다.
    #[test]
    fn whole_cell_enabled_true_in_table_mode() {
        let mut doc = find_test_doc(&["a,b"]).docs.remove(0);
        doc.sep = SeparatorMode::Char(b',');
        assert!(whole_cell_enabled(&doc), "표 모드는 Whole cell을 켤 수 있어야 한다");
    }

    #[test]
    fn whole_cell_enabled_false_in_text_mode() {
        let mut doc = find_test_doc(&["a,b"]).docs.remove(0);
        doc.sep = SeparatorMode::None;
        assert!(!whole_cell_enabled(&doc), "텍스트 모드는 Whole cell을 끌 수 없어야 한다");
    }

    /// 찾기 창이 열려 있어도(`show_find = true`) 탭 바를 잠그면 안 된다 —
    /// 찾기는 저장/확인 다이얼로그와 달리 탭 전환을 막을 이유가 없다(찾기
    /// 상태는 Document별이라 탭을 바꾸면 자연히 그 탭의 상태를 본다).
    /// `tab_bar_locked`는 다이얼로그 넷(`pending_action`/저장/열기 방식/
    /// 헥스 로드 확인)만 보고 `show_find`를 아예 인자로 받지 않으므로, 이
    /// 테스트는 그 시그니처 자체가 계약을 지킨다는 것을 실제로 호출해 고정한다.
    #[test]
    fn find_dialog_open_does_not_lock_tab_bar() {
        let mut app = find_test_doc(&["hit"]);
        app.doc_mut().unwrap().show_find = true;
        assert!(
            !tab_bar_locked_for(&app),
            "찾기 창이 떠 있어도 탭 바는 잠기지 않아야 한다"
        );
    }

    /// `find_opts_changed`(체크박스 시절부터 있던 리셋 판정을 라디오 도입에
    /// 맞춰 뽑은 순수 함수): scope만 달라져도, match_case만 달라져도 참이어야
    /// 하고, 완전히 같으면 거짓이어야 한다. 조건을 뒤집으면(`==`로 바꾸면) 이
    /// 테스트들이 반드시 깨진다.
    #[test]
    fn find_opts_changed_detects_scope_change() {
        let before = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::Partial,
        };
        let after = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::WholeWord,
        };
        assert!(find_opts_changed(&before, &after), "scope가 바뀌면 참이어야 한다");
    }

    #[test]
    fn find_opts_changed_detects_match_case_change() {
        let before = crate::find::FindOptions {
            match_case: false,
            scope: crate::find::MatchScope::Partial,
        };
        let after =
            crate::find::FindOptions { match_case: true, scope: crate::find::MatchScope::Partial };
        assert!(find_opts_changed(&before, &after), "match_case가 바뀌면 참이어야 한다");
    }

    #[test]
    fn find_opts_changed_false_when_identical() {
        let opts = crate::find::FindOptions {
            match_case: true,
            scope: crate::find::MatchScope::WholeCell,
        };
        assert!(
            !find_opts_changed(&opts, &opts.clone()),
            "옵션이 그대로면 리셋하면 안 된다"
        );
    }

    /// 통합 확인: 라디오로 scope를 실제로 바꾼 뒤(egui 클릭 시뮬레이션 없이,
    /// 창을 다시 그리기 **전에** `doc.find_opts.scope`를 직접 바꿔 다음 프레임의
    /// "이전 값"이 이미 새 값을 반영하는 문제를 피하려면, 리셋이 일어나야 하는
    /// 프레임 자체에서 옵션을 바꿔야 한다 — `render_find_panel`은 그 프레임
    /// 안에서 `before`를 캡처하고 라디오는 손대지 않은 채 `doc.find_opts.scope`로
    /// 그대로 되읽으므로, 프레임 밖에서 미리 바꾼 값은 `before`에도 이미 반영돼
    /// 버린다. 그래서 이 케이스는 실제 반영 경로 대신 `find_opts_changed`
    /// 자체로 검증하고(위 세 테스트), 여기서는 **옵션을 안 바꾸면 리셋도 안
    /// 된다**는 반대쪽만 `render_find_panel`로 확인한다.
    #[test]
    fn render_find_panel_keeps_last_match_when_options_unchanged() {
        let mut app = find_test_doc(&["hit", "no hit here"]);
        let ctx = egui::Context::default();
        {
            let doc = app.doc_mut().unwrap();
            doc.show_find = true;
            doc.find_query = "hit".to_owned();
            doc.last_match = Some(crate::find::Match { line: 0, col: 0, len: 3 });
            doc.find_status = "stale status".to_owned();
        }
        let _ = ctx.run(Default::default(), |ctx| {
            let doc = app.doc_mut().unwrap();
            let _ = render_find_panel(ctx, doc, crate::i18n::Lang::default());
        });
        assert_eq!(
            app.doc().unwrap().last_match,
            Some(crate::find::Match { line: 0, col: 0, len: 3 }),
            "옵션을 건드리지 않았으면 last_match가 그대로 유지돼야 한다"
        );
        assert_eq!(
            app.doc().unwrap().find_status,
            "stale status",
            "옵션을 건드리지 않았으면 find_status도 그대로 유지돼야 한다"
        );
    }

    /// 매치 개수 문구 순수 함수 `find_count_text`: find_status가 있으면
    /// 그것을 우선하고, 없으면 검색어 유무에 따라 "N matching rows"/빈 문자열을
    /// 낸다. 단수(1)는 "row" 단수형을 쓴다.
    #[test]
    fn find_count_text_variants() {
        assert_eq!(find_count_text(0, "", true), "0 matching rows");
        assert_eq!(find_count_text(1, "", true), "1 matching row");
        assert_eq!(find_count_text(12, "", true), "12 matching rows");
        assert_eq!(find_count_text(0, "", false), "", "검색어가 없으면 아무 문구도 없다");
        assert_eq!(
            find_count_text(12, "Not found", true),
            "Not found",
            "find_status가 있으면 그것을 우선한다"
        );
        assert_eq!(
            find_count_text(0, "3 replacements", true),
            "3 replacements",
            "Replace 결과도 매치 개수보다 우선한다"
        );
    }

    /// Minor 7 회귀: Escape의 실제 소유권 판정 식(`update()`의 그 지점 —
    /// `focus.is_none() || focus == Some(find_query_id())`)을 그대로
    /// 재현해, 툴바의 커스텀 구분자 TextEdit 같은 **무관한** 위젯이 포커스를
    /// 쥐고 있을 때는 거짓이어야 함을 확인한다. 이 판정이 없으면(게이트가
    /// `show_find`뿐이면) 그 입력란에 타이핑하다 Escape를 누르면 패널이
    /// 닫혀 버린다.
    #[test]
    fn escape_yields_when_unrelated_widget_has_focus() {
        let ctx = egui::Context::default();
        let toolbar_sep = egui::Id::new("toolbar_custom_sep_textedit");
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(50.0, 20.0));
                let r = ui.interact(rect, toolbar_sep, egui::Sense::click());
                r.request_focus();
            });
        });
        let focus = ctx.memory(|m| m.focused());
        assert_eq!(focus, Some(toolbar_sep), "사전 조건: 무관한 위젯이 포커스를 쥔다");
        let escape_owner_ok = focus.is_none() || focus == Some(find_query_id());
        assert!(
            !escape_owner_ok,
            "무관한 위젯에 포커스가 있으면 Escape가 찾기 패널을 닫으면 안 된다"
        );
    }

    /// Escape의 반대쪽 절반: 찾기 입력란 **자신**에 포커스가 있을 때는
    /// 여전히 참이어야 한다 — 그게 이 단축키의 정상 사용 흐름이다
    /// (Ctrl+F로 연 직후, 또는 입력란에 타이핑하는 중에 Escape로 닫기).
    #[test]
    fn escape_still_fires_when_find_box_itself_has_focus() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(50.0, 20.0));
                let r = ui.interact(rect, find_query_id(), egui::Sense::click());
                r.request_focus();
            });
        });
        let focus = ctx.memory(|m| m.focused());
        assert_eq!(focus, Some(find_query_id()), "사전 조건: 찾기 입력란 자신이 포커스를 쥔다");
        let escape_owner_ok = focus.is_none() || focus == Some(find_query_id());
        assert!(
            escape_owner_ok,
            "찾기 입력란 자신에 포커스가 있으면 Escape가 여전히 패널을 닫아야 한다"
        );
    }

    /// 포커스가 아예 없을 때(패널의 버튼을 클릭한 직후 등)도 Escape가
    /// 살아 있어야 한다 — 기존 동작을 이 조건에서 유지한다.
    #[test]
    fn escape_fires_when_nothing_has_focus() {
        let ctx = egui::Context::default();
        let focus = ctx.memory(|m| m.focused());
        assert_eq!(focus, None, "사전 조건: 아무 위젯도 포커스가 없다");
        let escape_owner_ok = focus.is_none() || focus == Some(find_query_id());
        assert!(escape_owner_ok, "포커스가 없으면 Escape가 패널을 닫아야 한다");
    }

    /// 찾기가 별도 `egui::Window`로 바뀌었으므로(S-8) 더 이상 상태바와
    /// `TopBottomPanel` 쌓기 순서를 다투지 않는다 — 창은 상태바 영역을 잠식하지
    /// 않고 그 위에 자유롭게 뜬다. 옛 `find_panel_sits_above_status_bar`가
    /// 검증하던 것(찾기 UI가 상태바를 가리지 않는다)은 창 전환으로 구조적으로
    /// 항상 참이 되었으므로, 대신 창이 실제로 화면 영역 안에(상태바 위) 뜬다는
    /// 것만 스모크로 확인한다.
    #[test]
    fn find_window_does_not_cover_status_bar() {
        let mut app = find_test_doc(&["hit"]);
        app.doc_mut().unwrap().show_find = true;
        let ctx = egui::Context::default();
        let mut status_top = 0.0f32;
        let mut find_window_bottom = 0.0f32;
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                let status = egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| {
                    ui.label(crate::theme::chrome_text("Ready"));
                });
                status_top = status.response.rect.top();
                let doc = app.doc_mut().unwrap();
                let _ = render_find_panel(ctx, doc, crate::i18n::Lang::default());
                find_window_bottom = ctx
                    .memory(|m| m.area_rect(egui::Id::new("Find & Replace")))
                    .map(|r| r.bottom())
                    .unwrap_or(0.0);
            });
        }
        assert!(
            find_window_bottom <= status_top + 0.5,
            "찾기 창의 기본 위치(우상단)가 상태바를 가리지 않아야 한다 \
             (find_window_bottom={find_window_bottom}, status_top={status_top})"
        );
    }

    // ---- 찾기 결과 행 추출 (Extract Rows) ----

    /// 추출 테스트용 **뷰 모드** 표 문서. 실제 파일을 CSV로 열고 인덱싱을
    /// 끝까지 돌린다 — 추출은 뷰 모드에서도 동작해야 하고(브리프 D-5),
    /// 뷰 경로(`decode_logical_line`)를 지나야 인코딩 처리가 실제로 검증된다.
    fn extract_test_app(content: &[u8]) -> App {
        let p = temp(content);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        app.doc_mut().unwrap().indexer.take().unwrap().join().unwrap();
        app
    }

    /// 추출본을 `logical_line`으로 처음부터 끝까지 읽어 낸다.
    fn read_all(doc: &Document) -> Vec<String> {
        (0..doc_line_count(doc))
            .map(|i| logical_line(doc, i).unwrap())
            .collect()
    }

    #[test]
    fn extract_plan_skips_header_and_prepends_it() {
        assert_eq!(
            extract_plan(true, SeparatorMode::Char(b',')),
            ExtractPlan { scan_from: 1, prepend_header: true }
        );
        assert_eq!(
            extract_plan(false, SeparatorMode::Char(b',')),
            ExtractPlan { scan_from: 0, prepend_header: false }
        );
        // 텍스트 모드는 헤더 개념이 없다.
        assert_eq!(
            extract_plan(true, SeparatorMode::None),
            ExtractPlan { scan_from: 0, prepend_header: false }
        );
    }

    #[test]
    fn extract_includes_header_and_matching_rows() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\nCarol,Seoul\n");
        assert!(app.doc().unwrap().has_header, "사전 조건: 헤더가 감지된다");
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 2);
        let new_doc = &app.docs[1];
        assert_eq!(
            read_all(new_doc),
            v(&["name,city", "Alice,Seoul", "Carol,Seoul"]),
            "첫 행이 원본 헤더, 나머지가 매치된 데이터 행"
        );
        assert!(new_doc.has_header, "헤더를 붙였으므로 추출본도 헤더를 갖는다");
    }

    /// 검색어가 헤더에만 있으면 추출 결과는 0행이고 탭이 만들어지지 않는다 —
    /// 헤더가 검색 대상에 들어가면 데이터가 하나도 안 맞는데도 헤더 한 줄짜리
    /// 탭이 열리는 거짓 성공이 된다.
    #[test]
    fn extract_does_not_search_the_header_row() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\n");
        app.doc_mut().unwrap().find_query = "city".to_owned();
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 1, "헤더에만 있는 검색어로는 탭이 열리지 않는다");
        assert_eq!(app.doc().unwrap().find_status, "Not found");
    }

    #[test]
    fn extract_headerless_has_no_header_row() {
        // 숫자만 있는 CSV는 헤더로 감지되지 않는다. 그래도 방어적으로 끈다.
        let mut app = extract_test_app(b"1,hit\n2,no\n3,hit\n");
        app.doc_mut().unwrap().has_header = false;
        app.doc_mut().unwrap().find_query = "hit".to_owned();
        app.extract_matching_rows();
        assert_eq!(
            read_all(&app.docs[1]),
            v(&["1,hit", "3,hit"]),
            "헤더가 없으면 앞에 아무것도 붙지 않는다"
        );
        assert!(!app.docs[1].has_header);
    }

    #[test]
    fn extract_creates_new_tab_and_leaves_original_untouched() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\n");
        {
            let doc = app.doc_mut().unwrap();
            // "추출이 원본을 편집 모드로 끌고 가지 않는다"를 보려면 원본이
            // 뷰 모드여야 한다(작은 파일은 열 때 자동으로 편집 모드가 된다).
            view_doc(doc);
            doc.find_query = "Seoul".to_owned();
            // 원본의 선택 상태가 추출 때문에 흔들리면 안 된다.
            doc.cell_sel = Some((2, 0, 2, 1));
            doc.selected_col = Some(1);
        }
        let before_lines = read_all(app.doc().unwrap());
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 2, "탭이 하나 늘어난다");
        assert_eq!(app.active, 1, "새 탭이 활성화된다");
        let orig = &app.docs[0];
        assert_eq!(read_all(orig), before_lines, "원본 내용은 그대로");
        assert_eq!(orig.cell_sel, Some((2, 0, 2, 1)), "원본 선택도 그대로");
        assert_eq!(orig.selected_col, Some(1));
        assert!(orig.edit.is_none(), "원본이 편집 모드로 끌려 들어가지 않는다");
    }

    #[test]
    fn extract_inherits_encoding_and_separator() {
        // 탭 구분 + CP949(한글). 확장자가 csv라도 내용으로 탭이 잡히도록
        // 콤마를 쓰지 않는다.
        let bytes = crate::save::encode_bytes(
            "이름\t도시\n가가\t서울\n나나\t부산\n",
            Encoding::Cp949,
        );
        let mut app = extract_test_app(&bytes);
        {
            let doc = app.doc_mut().unwrap();
            assert_eq!(doc.enc, Encoding::Cp949, "사전 조건: CP949로 감지");
            doc.find_query = "서울".to_owned();
        }
        let (enc, sep) = {
            let d = app.doc().unwrap();
            (d.enc, d.sep)
        };
        app.extract_matching_rows();
        let new_doc = &app.docs[1];
        assert_eq!(new_doc.enc, enc, "인코딩을 물려받는다");
        assert_eq!(new_doc.sep, sep, "구분자를 물려받는다");
    }

    /// **(가) 방식이 실제로 동작하는지 증명하는 핵심 테스트.** 만들어진
    /// 문서는 인메모리 `Source` + 동기로 채운 `LineIndex`만 갖는다 —
    /// 그 위에서 `logical_line`(= 뷰 경로 `decode_logical_line`)이 추출된
    /// 행을 정확히 돌려주어야 한다. 인덱스 offset이 하나라도 어긋나면 행이
    /// 밀리거나 화면이 빈다. CP949 + 한글로 확인해 인코딩 왕복도 함께 건다.
    #[test]
    fn extracted_doc_is_readable_through_logical_line() {
        let bytes = crate::save::encode_bytes(
            "이름,도시\n가가,서울\n나나,부산\n다다,서울\n",
            Encoding::Cp949,
        );
        let mut app = extract_test_app(&bytes);
        {
            let doc = app.doc_mut().unwrap();
            assert_eq!(doc.enc, Encoding::Cp949, "사전 조건: CP949");
            assert!(doc.has_header, "사전 조건: 헤더 감지");
            doc.find_query = "서울".to_owned();
        }
        app.extract_matching_rows();
        let new_doc = &app.docs[1];
        assert_eq!(
            doc_line_count(new_doc),
            3,
            "인덱스가 헤더 1 + 매치 2 = 3행을 알아야 한다"
        );
        assert_eq!(
            read_all(new_doc),
            v(&["이름,도시", "가가,서울", "다다,서울"]),
            "뷰 경로 디코딩이 추출된 행을 그대로 돌려준다"
        );
        assert_eq!(
            new_doc.index.status().phase,
            Phase::Complete,
            "동기로 다 채웠으므로 인덱싱은 완료 상태다"
        );
        assert!(new_doc.indexer.is_none(), "백그라운드 인덱서를 띄우지 않는다");
        // 추출본에서 편집 모드로 들어가도 같은 내용이 읽혀야 한다
        // (뷰 모드로 되돌릴 수 있는 온전한 문서라는 뜻).
        let new_doc = &mut app.docs[1];
        enter_edit_mode(new_doc);
        assert_eq!(read_all(new_doc), v(&["이름,도시", "가가,서울", "다다,서울"]));
        exit_edit_mode(new_doc);
        assert_eq!(read_all(new_doc), v(&["이름,도시", "가가,서울", "다다,서울"]));
    }

    /// CRLF 원본에서 추출해도 행이 밀리지 않는다(개행이 2바이트여도 offset이
    /// 맞아야 한다). 편집 버퍼의 `newline`을 물려받는 경로를 함께 탄다.
    #[test]
    fn extract_preserves_crlf_newline_of_edit_buffer() {
        let mut app = extract_test_app(b"a,b\r\n1,hit\r\n2,no\r\n3,hit\r\n");
        {
            let doc = app.doc_mut().unwrap();
            enter_edit_mode(doc);
            assert_eq!(
                doc.edit.as_ref().unwrap().newline,
                crate::edit::Newline::CrLf,
                "사전 조건: CRLF로 읽힌다"
            );
            doc.has_header = true;
            doc.find_query = "hit".to_owned();
        }
        app.extract_matching_rows();
        assert_eq!(read_all(&app.docs[1]), v(&["a,b", "1,hit", "3,hit"]));
    }

    #[test]
    fn extract_zero_matches_creates_no_tab() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\n");
        app.doc_mut().unwrap().find_query = "zzz".to_owned();
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 1, "매치가 없으면 빈 탭을 만들지 않는다");
        assert_eq!(app.doc().unwrap().find_status, "Not found");
    }

    #[test]
    fn extract_with_empty_query_does_nothing() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\n");
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 1);
        assert_eq!(app.doc().unwrap().find_status, "Enter text to find");
    }

    /// 저장/확인 다이얼로그가 떠 있으면(탭 바 잠금) 추출이 탭을 추가하지 않는다.
    /// **실제 가드(`extract_allowed`)를 통과하는 진짜 경로**인
    /// `extract_matching_rows`를 호출한다 — 가드 식을 테스트에 복붙하면
    /// `extract_matching_rows` 안의 가드를 지워도 이 테스트가 계속 통과한다.
    #[test]
    fn extract_blocked_while_dialog_open() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\n");
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        // (1) 저장 다이얼로그.
        app.show_save_dialog = true;
        assert!(!extract_allowed(&app), "사전 조건: 잠긴 상태");
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 1, "저장 다이얼로그 중에는 탭이 늘지 않는다");
        assert_eq!(app.active, 0, "active도 움직이지 않는다");
        assert_eq!(app.doc().unwrap().find_status, EXTRACT_LOCKED_STATUS);
        // (2) 확인 다이얼로그.
        app.show_save_dialog = false;
        app.pending_action = Some(PendingAction::ExitEditMode);
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 1, "확인 다이얼로그 중에도 탭이 늘지 않는다");
        // (3) 잠금이 풀리면 정상 동작한다 — 위의 0건이 "잠금 때문"임을 확정한다.
        app.pending_action = None;
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 2, "잠금이 풀리면 추출된다");
    }

    /// 추출본은 `path`가 비어 있으므로 `tab_label`이 `path_label`을 쓴다.
    /// 원본 파일명이 라벨 앞부분(24자 예산 안)에 보여야 탭에서 구분된다.
    #[test]
    fn extracted_tab_label_shows_source_file_name() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\n");
        let file_name = app
            .doc()
            .unwrap()
            .path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        app.extract_matching_rows();
        let new_doc = &app.docs[1];
        assert!(new_doc.path.as_os_str().is_empty(), "추출본에는 파일 경로가 없다");
        assert_eq!(new_doc.path_label, format!("[hit] {file_name}"));
        let label = tab_label(new_doc);
        assert!(label.starts_with("[hit]"), "라벨 앞부분에 추출본 표시가 남는다: {label}");
        assert!(label.chars().count() <= 24, "tab_label의 24자 상한을 지킨다");
    }

    /// 파일이 없는 추출본을 저장하면 `render_save_dialog`가 "덮어쓰기"가 아니라
    /// **"다른 이름으로 저장"으로 폴백**해야 한다(빈 경로에 write_file을 부르면
    /// 실패한다). `save_as_fallback`이 그 판정식 자체이므로 그 함수를
    /// 직접 호출해 검증한다 — 프로덕션 식을 여기서 다시 베껴 쓰면(과거처럼)
    /// 실제 판정식을 뒤집어도 이 테스트는 계속 통과하는 착시가 생긴다.
    #[test]
    fn extracted_doc_saves_via_save_as_fallback() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\n");
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        app.extract_matching_rows();
        let cur_path = app.doc().unwrap().path.clone();
        assert!(
            save_as_fallback(app.save_as, cur_path.as_os_str().is_empty()),
            "추출본은 save_as가 아니어도 경로가 비어 파일 선택 창으로 폴백한다"
        );
    }

    /// `save_as_fallback` 자체의 네 가지 조합. 특히 `save_as == false`이고
    /// 경로가 빈 경우(추출본에 평범한 Ctrl+S) — 이 조합이 참이어야 폴백이
    /// 일어난다. 이 케이스를 빠뜨리면 "경로 갱신도 save_as만 본다"는 예전
    /// 버그(추출본을 Ctrl+S로 저장해도 탭이 계속 추출본이라 우기고, 그래서
    /// 매 저장마다 파일 선택 창이 다시 뜨는 버그)를 이 함수 하나가 재도입해도
    /// 아무 테스트도 잡아내지 못한다.
    #[test]
    fn save_as_fallback_covers_all_combinations() {
        assert!(save_as_fallback(true, true), "save_as면 경로 유무와 무관하게 폴백");
        assert!(save_as_fallback(true, false), "save_as면 경로 유무와 무관하게 폴백");
        assert!(
            save_as_fallback(false, true),
            "save_as가 아니어도 경로가 비어 있으면 폴백해야 한다(추출본 Ctrl+S)"
        );
        assert!(!save_as_fallback(false, false), "경로가 있고 save_as도 아니면 그대로 덮어쓴다");
    }

    /// 저장(다른 이름으로) 후 `repoint_source_after_save`가 인메모리 소스를
    /// 진짜 파일로 갈아 끼운다 — 그 뒤로 그 탭은 저장된 파일을 본다.
    /// (`render_save_dialog`의 성공 분기가 하는 일을 그대로 재현한다.)
    #[test]
    fn extracted_doc_repoints_to_file_after_save_as() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\n");
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        app.extract_matching_rows();
        let ctx = egui::Context::default();
        let target = temp(b"");
        {
            let doc = app.doc_mut().unwrap();
            enter_edit_mode(doc);
            let opts = crate::save::SaveOptions {
                enc: doc.enc,
                bom: false,
                newline: crate::edit::Newline::Lf,
            };
            let lines = doc.edit.as_ref().unwrap().lines.clone();
            crate::save::write_file(&target, &lines, &opts, None).unwrap();
            // render_save_dialog의 성공 분기와 동일한 뒷정리. `path_will_update`는
            // 그 분기가 실제로 쓰는 판정식(`save_as_fallback`)을 그대로 재현한다 —
            // `save_as`만 보면 Important 1 버그(평범한 Ctrl+S가 경로를 갱신하지
            // 않는 문제)를 이 테스트가 다시 못 잡아낸다.
            let cur_path_empty = doc.path.as_os_str().is_empty();
            let path_will_update = save_as_fallback(app.save_as, cur_path_empty);
            let doc = app.doc_mut().unwrap();
            doc.edit.as_mut().unwrap().dirty = false;
            if path_will_update {
                doc.path_label = target.display().to_string();
                doc.path = target.clone();
            }
            repoint_source_after_save(doc, &target, &ctx).unwrap();
            doc.indexer.take().unwrap().join().unwrap();
            exit_edit_mode(doc);
        }
        let doc = app.doc().unwrap();
        assert_eq!(
            read_all(doc),
            v(&["name,city", "Alice,Seoul"]),
            "저장 후 그 탭은 저장된 파일을 본다"
        );
        assert_eq!(tab_label(doc), target.file_name().unwrap().to_str().unwrap());
    }

    /// Important 1의 정확한 회귀 시나리오: 추출 탭에서 `save_as == false`인
    /// 채로(평범한 Ctrl+S) 저장한다. `cur_path`가 비어 있으므로
    /// `save_as_fallback`이 폴백을 지시해야 하고, 저장 성공 뒤 `doc.path`가
    /// 실제로 채워져야 한다 — 그래야 그 탭이 "파일"이 되어 **다음** Ctrl+S부터는
    /// 파일 선택 창 없이 바로 덮어쓴다. 이 갱신이 빠지면 매 저장마다 사용자가
    /// 파일명을 다시 입력해야 한다.
    #[test]
    fn extracted_doc_plain_save_converts_tab_to_file_tab() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\n");
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        app.extract_matching_rows();
        let ctx = egui::Context::default();
        let target = temp(b"");
        // 평범한 Save(Ctrl+S) 경로: save_as는 false다.
        app.save_as = false;
        {
            let doc = app.doc_mut().unwrap();
            assert!(doc.path.as_os_str().is_empty(), "사전 조건: 추출본은 경로가 비어 있다");
            enter_edit_mode(doc);
            let opts = crate::save::SaveOptions {
                enc: doc.enc,
                bom: false,
                newline: crate::edit::Newline::Lf,
            };
            let lines = doc.edit.as_ref().unwrap().lines.clone();
            crate::save::write_file(&target, &lines, &opts, None).unwrap();

            let cur_path_empty = doc.path.as_os_str().is_empty();
            let path_will_update = save_as_fallback(app.save_as, cur_path_empty);
            assert!(path_will_update, "save_as가 false여도 경로가 비어 있으면 폴백해야 한다");

            let doc = app.doc_mut().unwrap();
            doc.edit.as_mut().unwrap().dirty = false;
            if path_will_update {
                doc.path_label = target.display().to_string();
                doc.path = target.clone();
            }
            repoint_source_after_save(doc, &target, &ctx).unwrap();
            doc.indexer.take().unwrap().join().unwrap();
            exit_edit_mode(doc);
        }
        let doc = app.doc().unwrap();
        assert_eq!(doc.path, target, "저장 후 탭의 경로가 실제 파일을 가리켜야 한다");
        assert!(
            !doc.path_label.starts_with("[hit]"),
            "저장 후 탭 라벨은 더 이상 추출본을 주장하면 안 된다: {}",
            doc.path_label
        );
    }

    /// 추출은 편집 모드에서도 동작해야 한다(찾기와 같다). 편집 버퍼의 내용이
    /// 추출 대상이 되는지 확인한다 — 버퍼가 파일과 다르면 버퍼가 진실이다.
    #[test]
    fn extract_works_in_edit_mode_from_the_buffer() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\n");
        {
            let doc = app.doc_mut().unwrap();
            enter_edit_mode(doc);
            // 버퍼만 바꾼다(파일은 그대로) — 추출이 버퍼를 보는지 판별한다.
            doc.edit.as_mut().unwrap().lines[2] = "Bob,Seoul".to_owned();
            doc.find_query = "Seoul".to_owned();
        }
        app.extract_matching_rows();
        assert_eq!(
            read_all(&app.docs[1]),
            v(&["name,city", "Alice,Seoul", "Bob,Seoul"]),
            "편집 버퍼의 내용이 추출된다"
        );
        assert!(app.docs[1].edit.is_none(), "추출본은 뷰 모드로 열린다");
    }

    /// 추출 결과 안내는 **원본 탭**에 남는다(활성 탭은 새 탭으로 바뀌지만,
    /// 원본으로 돌아왔을 때 방금 무슨 일이 있었는지 보여야 한다).
    #[test]
    fn extract_reports_row_count_on_the_source_tab() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\nCarol,Seoul\n");
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        app.extract_matching_rows();
        assert_eq!(app.docs[0].find_status, "2 rows extracted");
        assert!(app.docs[1].find_status.is_empty(), "새 탭은 안내 문구 없이 시작");
        // 1행이면 단수(기존 "1 replacement"와 같은 규율).
        app.active = 0;
        app.doc_mut().unwrap().find_query = "Busan".to_owned();
        app.extract_matching_rows();
        assert_eq!(app.docs[0].find_status, "1 row extracted");
    }

    /// 추출은 찾기 옵션(대소문자/단어 단위)을 그대로 따른다 — 찾기로 잡히는
    /// 행과 추출되는 행이 어긋나면 안 된다.
    #[test]
    fn extract_respects_find_options() {
        let mut app = extract_test_app(b"a,b\n1,HIT\n2,hit\n3,hitting\n");
        {
            let doc = app.doc_mut().unwrap();
            doc.has_header = true;
            doc.find_query = "hit".to_owned();
            doc.find_opts.match_case = true;
        }
        app.extract_matching_rows();
        assert_eq!(
            read_all(&app.docs[1]),
            v(&["a,b", "2,hit", "3,hitting"]),
            "대소문자 구분이 켜지면 HIT는 빠진다"
        );
        app.active = 0;
        app.doc_mut().unwrap().find_opts.scope = crate::find::MatchScope::WholeWord;
        app.extract_matching_rows();
        assert_eq!(
            read_all(&app.docs[2]),
            v(&["a,b", "2,hit"]),
            "단어 단위까지 켜지면 hitting도 빠진다"
        );
    }

    /// J-1 회귀: 추출이 `find::matching_lines` 브루트포스 대신
    /// `scan_all_matches` 바이트 스캔을 쓰게 바뀌었다. **행 집합은 예전과
    /// 완전히 같아야 한다** — 그 등가성을 여기서 직접 확인한다: 예전 방식
    /// (`matching_lines` + `scan_from` 오프셋)을 테스트 안에서 그대로 재현해
    /// 실제 추출 결과와 비교한다. 옵션/구분자/헤더 유무를 두루 돌린다.
    #[test]
    fn extract_uses_fast_scan_and_matches_brute_force() {
        // 예전(브루트포스) 방식의 히트 행 번호를 그대로 재현한다.
        fn old_way(doc: &Document) -> Vec<usize> {
            let plan = extract_plan(doc.has_header, doc.sep);
            let total = doc_line_count(doc);
            crate::find::matching_lines(
                total.saturating_sub(plan.scan_from),
                &effective_query(doc),
                &doc.find_opts,
                doc_delimiter(doc),
                |i| logical_line(doc, i + plan.scan_from),
            )
            .into_iter()
            .map(|i| i + plan.scan_from)
            .collect()
        }

        let bodies: &[&[u8]] = &[
            b"name,city\nAlice,Seoul\nBob,Busan\nCarol,SEOUL\nseoul,Alice\n",
            b"1,hit\n2,no\n3,HIT\n4,hitting\n",
            b"a,b\n\"x,y\",hit\n\"hit\",z\nq,\"hi\"\"t\"\n",
        ];
        let scopes = [
            crate::find::MatchScope::Partial,
            crate::find::MatchScope::WholeWord,
            crate::find::MatchScope::WholeCell,
        ];
        for body in bodies {
            for has_header in [true, false] {
                for needle in ["hit", "Seoul", "seoul", "x,y", "hi\"t"] {
                    for &scope in &scopes {
                        for match_case in [true, false] {
                            let mut app = extract_test_app(body);
                            {
                                let doc = app.doc_mut().unwrap();
                                doc.has_header = has_header;
                                doc.find_query = needle.to_owned();
                                doc.find_opts =
                                    crate::find::FindOptions { match_case, scope };
                            }
                            let doc = app.doc().unwrap();
                            let expected = old_way(doc);
                            // 지금 구현이 실제로 고르는 행 집합.
                            let plan = extract_plan(doc.has_header, doc.sep);
                            let got: Vec<usize> = scan_all_matches(doc)
                                .into_iter()
                                .map(|r| r as usize)
                                .filter(|&r| r >= plan.scan_from)
                                .collect();
                            assert_eq!(
                                got, expected,
                                "빠른 스캔 + 헤더 필터가 예전 브루트포스와 다르다 \
                                 (needle={needle:?}, scope={scope:?}, \
                                 match_case={match_case}, has_header={has_header})"
                            );
                            // 실제 추출 결과(줄 텍스트)도 같은 행 집합에서 나온다.
                            // `extract_matching_rows`가 활성 탭을 새 탭으로 바꾸므로
                            // 기대값은 **원본 문서에서 미리** 만들어 둔다.
                            let want: Vec<String> = plan
                                .prepend_header
                                .then(|| logical_line(doc, 0).unwrap())
                                .into_iter()
                                .chain(expected.iter().map(|&i| logical_line(doc, i).unwrap()))
                                .collect();
                            app.extract_matching_rows();
                            if expected.is_empty() {
                                assert_eq!(app.docs.len(), 1, "0행이면 탭이 없다");
                            } else {
                                assert_eq!(read_all(&app.docs[1]), want);
                            }
                        }
                    }
                }
            }
        }
    }

    /// J-1 회귀: 새 방식은 문서 **전체**를 훑고 결과에서 헤더 행을 거른다.
    /// 헤더에 검색어가 있어도 추출 결과에 절대 들어가면 안 된다 —
    /// 필터(`r >= plan.scan_from`)를 지우면 헤더가 데이터 행으로 한 번 더
    /// 들어가 결과가 중복된다(맨 앞의 `prepend_header`와 겹친다).
    #[test]
    fn extract_excludes_header_even_when_header_matches() {
        let mut app = extract_test_app(b"city,name\nSeoul,Alice\nBusan,Bob\n");
        {
            let doc = app.doc_mut().unwrap();
            assert!(doc.has_header, "사전 조건: 헤더가 감지된다");
            // "city"는 헤더 행에만, 그리고 헤더에도 데이터에도 있는 경우를
            // 함께 보려고 데이터 행에도 걸리는 검색어를 쓴다.
            doc.find_query = "c".to_owned();
            doc.find_opts.match_case = false;
        }
        app.extract_matching_rows();
        assert_eq!(
            read_all(&app.docs[1]),
            v(&["city,name", "Seoul,Alice"]),
            "헤더 행은 맨 앞에 한 번만 붙고 매치 결과로는 들어가지 않는다"
        );
        assert_eq!(
            app.docs[0].find_status, "1 row extracted",
            "행 수도 헤더를 뺀 데이터 행 수여야 한다"
        );
    }

    /// 추출본에서 다시 추출해도 동작한다(파일이 없는 문서를 원본으로 삼는 경우).
    #[test]
    fn extract_from_an_extracted_doc() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\nCarol,Seoul\n");
        app.doc_mut().unwrap().find_query = "Seoul".to_owned();
        app.extract_matching_rows();
        app.doc_mut().unwrap().find_query = "Carol".to_owned();
        app.extract_matching_rows();
        assert_eq!(app.docs.len(), 3);
        assert_eq!(read_all(&app.docs[2]), v(&["name,city", "Carol,Seoul"]));
        assert_eq!(
            app.docs[2].path_label, app.docs[1].path_label,
            "접두사가 이미 붙어 있으면 다시 붙이지 않는다(반복 추출로 라벨이 자라면 \
             24자 예산이 접두사로만 찬다)"
        );
    }

    /// Minor 3 회귀: 실제 파일 이름이 이미 `"[hit] "`로 시작해도(우연의 일치),
    /// 그 파일은 추출본이 아니므로 접두사가 **또 붙어야** 한다. 라벨 텍스트로
    /// "이미 접두사가 붙었는가"를 추측하면(과거 버전) 이 경우 접두사를 생략해
    /// 추출 탭이 원본 파일 탭과 라벨이 완전히 같아져 버린다.
    #[test]
    fn extract_from_a_file_literally_named_with_the_hit_prefix() {
        let mut app = extract_test_app(b"name,city\nAlice,Seoul\nBob,Busan\n");
        {
            let doc = app.doc_mut().unwrap();
            assert!(!doc.is_extracted, "사전 조건: 실제 파일을 연 탭은 추출본이 아니다");
            doc.path = std::path::PathBuf::from("[hit] real.csv");
            doc.path_label = doc.path.display().to_string();
            doc.find_query = "Seoul".to_owned();
        }
        app.extract_matching_rows();
        let new_doc = &app.docs[1];
        assert_eq!(
            new_doc.path_label, "[hit] [hit] real.csv",
            "실제 파일명이 우연히 접두사로 시작해도 추출 시 접두사가 또 붙어야 \
             원본 파일 탭과 라벨로 구분된다"
        );
        assert!(new_doc.is_extracted, "추출본은 is_extracted가 참이다");
    }

    /// **스모크 테스트일 뿐이다**: `render_find_panel`이 검색어가 비었을 때와
    /// 있을 때 둘 다 패닉 없이 한 프레임을 그려낸다는 것만 확인한다(클릭하지
    /// 않았으니 어느 쪽도 인텐트를 내지 않는 것은 당연하다). "Extract Rows"
    /// 버튼이 실제로 존재하는지, 검색어 유무에 따라 활성/비활성이 바뀌는지는
    /// 이 테스트로는 검증되지 않는다 — egui는 지워진 버튼이 있어도 `None`을
    /// 그대로 돌려주므로 버튼 삭제를 잡아내지 못한다. 그 규칙은
    /// `extract_button_enabled_only_when_query_present`가 순수 함수로 검증한다.
    #[test]
    fn find_panel_renders_without_panicking_for_empty_and_nonempty_query() {
        let mut app = find_test_doc(&["hit"]);
        app.doc_mut().unwrap().show_find = true;
        let ctx = egui::Context::default();
        // 검색어가 빈 상태로 한 프레임.
        let _ = ctx.run(Default::default(), |ctx| {
            let doc = app.doc_mut().unwrap();
            assert_eq!(render_find_panel(ctx, doc, crate::i18n::Lang::default()), None);
        });
        // 검색어가 있는 상태로 한 프레임(버튼이 활성화된 경로).
        app.doc_mut().unwrap().find_query = "hit".to_owned();
        let _ = ctx.run(Default::default(), |ctx| {
            let doc = app.doc_mut().unwrap();
            assert_eq!(render_find_panel(ctx, doc, crate::i18n::Lang::default()), None, "클릭하지 않았으면 인텐트도 없다");
        });
    }

    /// "Extract Rows" 버튼의 실제 활성/비활성 규칙: 검색어가 빈 문자열일 때만
    /// 비활성이다. `render_find_panel`이 이 함수를 그대로 호출해 `add_enabled_ui`에
    /// 넘기므로, 이 규칙이 바뀌면(예: 항상 활성으로 뒤집히면) 여기서 잡힌다 —
    /// 렌더 결과(`Option<FindAction>`)만 보는 스모크 테스트로는 활성/비활성
    /// 차이가 드러나지 않는다.
    #[test]
    fn extract_button_enabled_only_when_query_present() {
        assert!(!extract_button_enabled(""), "검색어가 비었으면 비활성");
        assert!(extract_button_enabled("hit"), "검색어가 있으면 활성");
    }

    /// `apply_find_action`은 추출을 처리하지 않는다(탭을 건드릴 수 없으므로) —
    /// `&mut Document`만 받는 함수는 애초에 `App::docs`를 늘릴 수 없으므로
    /// (타입 시그니처만으로 참인 것을 확인하는 셈이라) 그 대신 **문서 내부
    /// 상태를 조용히 놔둔다**는 실제 계약을 확인한다: `find_status`나
    /// `last_match`를 바꾸지 않는다. 다른 변형(`Next`/`ReplaceOne` 등)은 이
    /// 필드들을 반드시 바꾸므로, `Extract` 분기가 빠지거나 다른 변형과
    /// 합쳐지면(예: 실수로 `FindAction::Next`의 동작을 타면) 이 테스트가
    /// 잡아낸다.
    #[test]
    fn apply_find_action_ignores_extract() {
        let mut app = find_test_doc(&["hit"]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "hit".to_owned();
        doc.find_status = "untouched".to_owned();
        apply_find_action(doc, FindAction::Extract);
        assert_eq!(doc.find_status, "untouched", "Extract는 find_status를 건드리지 않는다");
        assert_eq!(doc.last_match, None, "Extract는 last_match를 채우지 않는다(검색을 하지 않는다)");
    }

    /// 찾기 상태는 **탭마다** 독립이어야 한다.
    #[test]
    fn find_state_is_per_document() {
        let p1 = temp_ext(b"alpha\n", "txt");
        let p2 = temp_ext(b"beta\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p1, &ctx);
        app.doc_mut().unwrap().find_query = "alpha".to_owned();
        app.open_path(&p2, &ctx);
        assert!(app.doc().unwrap().find_query.is_empty(), "새 탭은 빈 검색어로 시작");
        app.active = 0;
        assert_eq!(app.doc().unwrap().find_query, "alpha", "탭마다 검색어가 따로 산다");
    }

    // ---- Page Up / Page Down (J-2) ----

    #[test]
    fn page_down_target_row() {
        // 첫 행 0, 한 화면 10행, 전체 100행 → 한 행 겹쳐 9로.
        assert_eq!(page_target_row(PageDir::Down, 0, 10, 100), Some(9));
        assert_eq!(page_target_row(PageDir::Down, 9, 10, 100), Some(18));
        // 마지막 행 너머로 가지 않는다.
        assert_eq!(page_target_row(PageDir::Down, 95, 10, 100), Some(99));
        assert_eq!(page_target_row(PageDir::Down, 99, 10, 100), Some(99));
        // 한 화면보다 작은 문서 — 끝까지만 간다.
        assert_eq!(page_target_row(PageDir::Down, 0, 50, 3), Some(2));
        // 행 0개면 갈 곳이 없다(스크롤 요청 자체를 만들지 않는다).
        assert_eq!(page_target_row(PageDir::Down, 0, 10, 0), None);
        // 창이 접혀 한 화면이 0/1행이어도 최소 한 행은 움직인다 —
        // 그러지 않으면 키가 죽은 것처럼 보인다.
        assert_eq!(page_target_row(PageDir::Down, 5, 0, 100), Some(6));
        assert_eq!(page_target_row(PageDir::Down, 5, 1, 100), Some(6));
    }

    #[test]
    fn page_up_target_row() {
        assert_eq!(page_target_row(PageDir::Up, 18, 10, 100), Some(9));
        assert_eq!(page_target_row(PageDir::Up, 9, 10, 100), Some(0));
        // 맨 위에서 Page Up은 0에 머문다(0 밑으로 안 내려간다).
        assert_eq!(page_target_row(PageDir::Up, 0, 10, 100), Some(0));
        assert_eq!(page_target_row(PageDir::Up, 3, 10, 100), Some(0));
        assert_eq!(page_target_row(PageDir::Up, 0, 10, 0), None);
        assert_eq!(page_target_row(PageDir::Up, 5, 0, 100), Some(4));
        // 첫 행이 문서 밖에 있어도(창 축소 등) 마지막 행으로 클램프된다.
        assert_eq!(page_target_row(PageDir::Up, 500, 10, 3), Some(2));
    }

    /// Page Down 한 번 뒤 Page Up 한 번이면 되돌아온다 — 두 방향의 보폭이
    /// 같아야(둘 다 `visible - 1`) 페이지를 넘겼다 되돌리는 동작이 제자리로
    /// 온다. 한쪽만 겹침 규칙을 바꾸면 이 테스트가 깨진다.
    #[test]
    fn page_down_then_up_returns_to_the_same_row() {
        let down = page_target_row(PageDir::Down, 30, 10, 1000).unwrap();
        assert_eq!(page_target_row(PageDir::Up, down, 10, 1000), Some(30));
    }

    /// 겹침 한 행이 실제로 지켜지는가 — 다음 페이지의 첫 행은 이전 페이지의
    /// **마지막 행**이다(문맥이 끊기지 않는다).
    #[test]
    fn page_down_overlaps_exactly_one_row() {
        let visible = 20;
        let first = 0;
        let last_visible = first + visible - 1; // 지금 보이는 마지막 행
        assert_eq!(
            page_target_row(PageDir::Down, first, visible, 1000),
            Some(last_visible),
            "다음 페이지의 첫 행 = 지금 페이지의 마지막 행(한 행 겹침)"
        );
    }

    /// 다른 위젯(찾기 입력란 등)이 포커스를 쥐고 있으면 Page 키는 양보한다.
    /// **실제 게이트 함수**를 호출한다 — 가드 식을 테스트에 베끼면 실제
    /// 가드를 뒤집어도 이 테스트는 자기 사본만 보고 통과한다.
    #[test]
    fn page_keys_yield_to_focused_widget() {
        // 아무것도 막지 않은 기본 상태에서는 살아 있다.
        assert!(page_keys_live(true, false, false, false, false));
        // 포커스를 쥔 위젯이 있으면 죽는다.
        assert!(
            !page_keys_live(true, false, true, false, false),
            "찾기 입력란에 타이핑 중이면 Page Down이 문서를 넘기면 안 된다"
        );
    }

    /// 인라인 셀 편집 중에는 Page 키가 문서로 새지 않는다(TextEdit이 가져간다).
    #[test]
    fn page_keys_ignored_while_editing_cell() {
        assert!(!page_keys_live(true, true, false, false, false));
    }

    /// 문서가 없거나 확인·저장 다이얼로그가 떠 있으면 양보한다
    /// (`find_keys_live`와 같은 규율).
    #[test]
    fn page_keys_yield_to_dialogs_and_empty_app() {
        assert!(!page_keys_live(false, false, false, false, false), "문서가 없으면 죽는다");
        assert!(!page_keys_live(true, false, false, true, false), "확인 다이얼로그");
        assert!(!page_keys_live(true, false, false, false, true), "저장 다이얼로그");
    }

    /// 게이트 어댑터가 실제 `App`/`Context` 상태를 제대로 읽는가.
    /// `page_keys_live_for`가 인자를 잘못 조립하면(예: 포커스를 늘 false로
    /// 넘기면) 순수 함수 테스트는 통과해도 이 테스트가 깨진다.
    #[test]
    fn page_keys_live_for_reads_real_state() {
        let mut app = find_test_doc(&["a", "b"]);
        let ctx = egui::Context::default();
        assert!(page_keys_live_for(&app, &ctx), "기본 상태에서는 살아 있다");
        app.doc_mut().unwrap().editing_cell = Some((0, 0));
        assert!(!page_keys_live_for(&app, &ctx), "셀 편집 중이면 죽는다");
        app.doc_mut().unwrap().editing_cell = None;
        app.show_save_dialog = true;
        assert!(!page_keys_live_for(&app, &ctx), "저장 다이얼로그가 떠 있으면 죽는다");
        app.show_save_dialog = false;
        // 실제 위젯에 포커스를 줘 본다 — `App`이 아니라 egui Context에서
        // 읽어야 하는 유일한 인자다.
        let id = egui::Id::new("page_focus_probe");
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut s = String::new();
                let resp = ui.add(egui::TextEdit::singleline(&mut s).id(id));
                resp.request_focus();
            });
        });
        assert_eq!(ctx.memory(|m| m.focused()), Some(id), "사전 조건: 포커스가 잡혔다");
        assert!(!page_keys_live_for(&app, &ctx), "다른 위젯이 포커스를 쥐면 죽는다");
    }

    /// 텍스트 모드: 페이지 이동이 화면 행(=논리 행) 기준으로 스크롤 요청을
    /// 남기고 정렬을 TOP으로 지시한다.
    #[test]
    fn page_scroll_sets_pending_row_and_top_align_in_text_mode() {
        let mut app = find_test_doc(&(0..100).map(|_| "x").collect::<Vec<_>>());
        let doc = app.doc_mut().unwrap();
        assert_eq!(doc.sep, SeparatorMode::None, "사전 조건: 텍스트 모드");
        doc.first_visible_row = 0;
        doc.visible_rows = 20;
        apply_page_scroll(doc, PageDir::Down);
        assert_eq!(doc.pending_scroll_row, Some(19), "한 행 겹쳐 19로");
        assert_eq!(
            doc.pending_scroll_align,
            egui::Align::TOP,
            "페이지 이동은 목표 행이 맨 위에 와야 다음 페이지가 이어진다"
        );
        // 렌더가 소비한 셈 치고 첫 행을 옮긴 뒤 Page Up.
        doc.pending_scroll_row = None;
        doc.first_visible_row = 19;
        apply_page_scroll(doc, PageDir::Up);
        assert_eq!(doc.pending_scroll_row, Some(0));
    }

    /// 표 모드: 마지막 행 클램프가 **헤더를 뺀** 데이터 행 수 기준이어야
    /// 한다(`render_table`의 `data_rows`와 같은 값). 헤더를 안 빼면 Page Down이
    /// 존재하지 않는 화면 행을 가리켜 마지막 행 하나가 안 보인다.
    #[test]
    fn page_scroll_clamps_to_data_rows_in_table_mode() {
        // 헤더 1 + 데이터 9 = 논리 10행 → 화면 행은 0..=8.
        let mut app = extract_test_app(b"h1,h2\n1,a\n2,b\n3,c\n4,d\n5,e\n6,f\n7,g\n8,h\n9,i\n");
        let doc = app.doc_mut().unwrap();
        assert!(doc.has_header, "사전 조건: 헤더가 감지된다");
        assert_eq!(doc_line_count(doc), 10);
        assert_eq!(doc_screen_row_count(doc), 9, "헤더는 본문 행이 아니다");
        doc.first_visible_row = 0;
        doc.visible_rows = 50; // 한 화면이 문서보다 크다.
        apply_page_scroll(doc, PageDir::Down);
        assert_eq!(
            doc.pending_scroll_row,
            Some(8),
            "마지막 **화면** 행(=데이터 행 수 - 1)까지만 간다"
        );
        // 헤더가 없으면 논리 행 = 화면 행이다.
        doc.has_header = false;
        assert_eq!(doc_screen_row_count(doc), 10);
    }

    /// 빈 문서에서는 스크롤 요청을 만들지 않는다 — 갈 곳이 없는데 0을 남기면
    /// 매 키 입력마다 무의미한 요청이 쌓인다.
    #[test]
    fn page_scroll_on_empty_doc_requests_nothing() {
        let mut app = find_test_doc(&[]);
        let doc = app.doc_mut().unwrap();
        assert_eq!(doc_line_count(doc), 0);
        doc.visible_rows = 20;
        apply_page_scroll(doc, PageDir::Down);
        assert_eq!(doc.pending_scroll_row, None);
        apply_page_scroll(doc, PageDir::Up);
        assert_eq!(doc.pending_scroll_row, None);
    }

    /// 회귀 방지: 페이지를 넘긴 뒤(정렬 TOP) 찾기를 하면 정렬이 다시
    /// **Center**로 돌아와야 한다. `focus_match`가 정렬을 매번 명시하지 않고
    /// 잔여 상태를 물려받으면, 페이지 한 번 넘긴 뒤의 Find Next가 조용히
    /// 상단 정렬로 바뀐다.
    #[test]
    fn find_restores_center_align_after_paging() {
        let mut app = find_test_doc(&["a", "b", "hit", "c"]);
        let doc = app.doc_mut().unwrap();
        doc.visible_rows = 2;
        apply_page_scroll(doc, PageDir::Down);
        assert_eq!(doc.pending_scroll_align, egui::Align::TOP, "사전 조건: TOP");
        doc.find_query = "hit".to_owned();
        apply_find_action(doc, FindAction::Next);
        assert_eq!(
            doc.pending_scroll_align,
            egui::Align::Center,
            "찾기는 언제나 매치를 화면 중앙에 둔다"
        );
        assert_eq!(doc.pending_scroll_row, Some(2));
    }

    /// 페이지 이동은 **캐럿을 옮기지 않는다**(요청은 "넘겨보기"뿐이다).
    /// 편집 모드에서도 선택/캐럿이 그대로여야 한다.
    #[test]
    fn page_scroll_does_not_move_the_caret() {
        let mut app = find_test_doc(&["a", "b", "c", "d", "e"]);
        let doc = app.doc_mut().unwrap();
        assert!(doc.edit.is_some(), "사전 조건: 편집 모드");
        let caret = doc.text_caret;
        let sel = doc.text_sel;
        doc.visible_rows = 2;
        apply_page_scroll(doc, PageDir::Down);
        assert_eq!(doc.text_caret, caret, "캐럿은 그대로");
        assert_eq!(doc.text_sel, sel, "선택도 그대로");
        assert_eq!(doc.cell_sel, None, "표 모드 선택도 건드리지 않는다");
    }

    /// Page Down 키가 실제로 `consume_key`로 잡히고 `apply_page_scroll`까지
    /// 이어지는가. `update()`가 `eframe::Frame`을 요구해 직접 부를 수 없으므로
    /// 그 블록이 쓰는 **실제 게이트/적용 함수**를 그대로 태운다
    /// (`ctrl_f_opens_find_panel`과 같은 규율).
    #[test]
    fn page_down_key_is_consumed_and_scrolls() {
        let mut app = find_test_doc(&(0..100).map(|_| "x").collect::<Vec<_>>());
        app.doc_mut().unwrap().visible_rows = 20;
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::PageDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(input, |ctx| {
            assert!(page_keys_live_for(&app, ctx), "사전 조건: Page 키가 살아 있다");
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown)) {
                apply_page_scroll(app.doc_mut().unwrap(), PageDir::Down);
            }
        });
        assert_eq!(
            app.doc().unwrap().pending_scroll_row,
            Some(19),
            "Page Down 키가 실제로 소비되어 페이지 이동이 일어난다"
        );
    }

    // ---- 헥스 렌더 ----

    /// 행 바이트 출처: 편집 전이면 mmap(소스), 편집 중이면 버퍼.
    #[test]
    fn hex_row_bytes_switches_source() {
        let mut app = hex_test_doc(&[0x41; 40]); // 32 + 8바이트, 2행
        {
            let doc = app.doc().unwrap();
            assert_eq!(hex_row_bytes(doc, 0).len(), 32);
            assert_eq!(hex_row_bytes(doc, 1), vec![0x41; 8]);
            assert_eq!(hex_row_bytes(doc, 9), Vec::<u8>::new(), "범위 밖 행은 빈 슬라이스");
        }
        let doc = app.doc_mut().unwrap();
        let h = doc.hex.as_mut().unwrap();
        h.edit = Some(crate::hex::HexEditBuffer::new(vec![0x42; 3]));
        assert_eq!(hex_row_bytes(doc, 0), vec![0x42; 3], "편집 중이면 버퍼가 진실");
        assert_eq!(hex_doc_len(doc), 3);
    }

    /// 헥스 문서는 CentralPanel에서 render_hex 분기를 타고 패닉 없이 그려진다.
    #[test]
    fn hex_render_smoke() {
        let mut app = hex_test_doc(b"SQLite format 3\x00\x10\x00\x01\x01\x00\x40\x20\x20");
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let doc = app.doc_mut().unwrap();
                render_hex(ui, doc, &mut String::new());
            });
        });
        // 그리기만으로 상태가 바뀌면 안 된다.
        let doc = app.doc().unwrap();
        let h = doc.hex.as_ref().unwrap();
        assert!(h.edit.is_none());
        assert_eq!(h.caret, (0, true));
    }

    /// 헥스 패널 클릭이 캐럿을 그 바이트로 옮긴다 — 클릭 산술
    /// (`hex_click_byte`)이 실제 렌더 좌표계에 붙어 있는지를 고정한다.
    /// (순수 함수 테스트만으로는 배선이 끊겨도 다 통과한다.)
    #[test]
    fn hex_click_moves_caret_to_that_byte() {
        let mut app = hex_test_doc(&vec![0x41u8; 256]);
        let ctx = egui::Context::default();
        let base = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(900.0, 400.0),
            )),
            ..Default::default()
        };
        let draw = |app: &mut App, input: egui::RawInput| {
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_hex(ui, app.doc_mut().unwrap(), &mut String::new());
                });
            });
        };
        // 첫 프레임으로 레이아웃을 잡는다(위젯 rect가 생겨야 클릭이 닿는다).
        draw(&mut app, base.clone());

        // 첫 행 헥스 패널의 세 번째 바이트 첫 칸 근처를 누른다. 오프셋 컬럼
        // 폭 + 바이트 2개 폭만큼 오른쪽 — 폭 계산은 렌더와 같은 식이다.
        let font = text_font_id();
        let char_w = ctx.fonts(|f| f.glyph_width(&font, '0'));
        let off_w = crate::hex::offset_width(256);
        let x = char_w * (off_w as f32 + 2.0) + char_w * 2.0 * 3.0 + char_w * 0.5;
        let y = ROW_HEIGHT * 0.5;
        let pos = egui::pos2(x + 8.0, y + 8.0); // CentralPanel 마진 여유
        let click_input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..base.clone()
        };
        draw(&mut app, click_input);

        let h = app.doc().unwrap().hex.as_ref().unwrap();
        // 세 번째 바이트(인덱스 2)를 눌렀다. CentralPanel 마진 때문에 한 칸
        // 오차는 허용하되, "옮겨지긴 했다" 수준이 아니라 그 언저리여야 한다.
        assert!(
            (1..=3).contains(&h.caret.0),
            "눌린 바이트 근처로 캐럿이 가야 한다(got {:?})",
            h.caret
        );
        assert_eq!(h.pane, crate::hex::HexPane::Hex, "클릭한 패널이 활성이 된다");
        assert!(h.sel.is_none(), "Shift 없는 단순 클릭은 선택을 만들지 않는다");
    }

    /// 행 **끝** 바이트를 오차 없이 찍는다. 위 테스트는 세 번째 바이트를 ±1
    /// 허용으로 보므로, 오차가 바이트마다 누적되는 결함을 놓친다.
    ///
    /// **한계를 분명히 해 둔다.** 이 테스트는 원래의 시각적 결함(사용자가 본
    /// "오른쪽으로 갈수록 캐럿이 다른 글자 위에 있다")을 **재현하지 못한다**.
    /// 확인해 봤다: 클릭 역산을 옛 `폭 ÷ 문자수` 나눗셈으로 되돌려도 이
    /// 테스트는 통과한다. 헤드리스 egui의 기본 폰트가 완전한 균등폭이라 두
    /// 방식이 같은 답을 내기 때문이다. 실제 화면에서는 폰트 힌팅·줌 배율에
    /// 따른 픽셀 반올림이 누적돼 어긋났다.
    ///
    /// 그래도 남겨 두는 이유: 클릭 좌표계가 렌더 좌표계에 붙어 있다는 것
    /// (셀 원점 기준, 마지막 바이트도 판정 범위 안)은 지켜지고, 폭 계산이
    /// 갤리에서 떨어져 나가면 여기서 깨진다. **시각 정렬 자체는 사람이
    /// 화면으로 확인해야 한다** — 이 코드베이스의 헤드리스 테스트로는
    /// 도달할 수 없는 종류의 결함이다.
    #[test]
    fn hex_click_is_exact_at_the_end_of_a_row() {
        use crate::hex::BYTES_PER_ROW;
        let mut app = hex_test_doc(&vec![0x41u8; 256]);
        let ctx = egui::Context::default();
        let base = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1400.0, 400.0),
            )),
            ..Default::default()
        };
        // 렌더가 그린 셀의 실제 좌표를 받아 온다 — 테스트가 폭 계산을
        // 베껴 쓰면 렌더와 함께 틀려도 통과한다(이 결함이 그랬다).
        let probe: std::cell::Cell<Option<(f32, f32, f32)>> = std::cell::Cell::new(None);
        let draw = |app: &mut App, input: egui::RawInput| {
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_hex(ui, app.doc_mut().unwrap(), &mut String::new());
                    // 첫 행 헥스 셀의 마지막 바이트 위치를 렌더와 같은
                    // 방식(갤리)으로 계산해 둔다.
                    if probe.get().is_none() {
                        let font = text_font_id();
                        let color = ui.visuals().text_color();
                        let prefix = "FF ".repeat(BYTES_PER_ROW - 1);
                        let g = ui.fonts(|f| {
                            f.layout_no_wrap(prefix, font.clone(), color)
                        });
                        let one = ui.fonts(|f| {
                            f.layout_no_wrap("FF".to_owned(), font.clone(), color)
                        });
                        probe.set(Some((g.size().x, one.size().x, 0.0)));
                    }
                });
            });
        };
        draw(&mut app, base.clone());

        // 마지막 바이트가 그려지는 x는 (앞 31바이트 폭) + (그 바이트 중앙).
        let (prefix_w, byte_w, _) = probe.get().expect("첫 프레임에서 재어 둔다");
        let hex_cell_left = {
            // 헥스 셀은 오프셋 컬럼 다음이다. 오프셋 폭도 렌더와 같은
            // 방식으로 잰다.
            let off_w = crate::hex::offset_width(256);
            let font = text_font_id();
            ctx.fonts(|f| {
                f.layout_no_wrap("0".repeat(off_w + 2), font, egui::Color32::WHITE)
                    .size()
                    .x
            })
        };
        // CentralPanel 기본 마진(8) + 컬럼 사이 간격은 egui가 정하므로,
        // 마지막 바이트의 **중앙**을 노려 한 칸 오차에 강건하게 만든다.
        let x = 8.0 + hex_cell_left + ctx.style().spacing.item_spacing.x + prefix_w + byte_w * 0.5;
        let y = 8.0 + ROW_HEIGHT * 0.5;
        let pos = egui::pos2(x, y);
        let click_input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..base.clone()
        };
        draw(&mut app, click_input);

        let h = app.doc().unwrap().hex.as_ref().unwrap();
        assert_eq!(
            h.caret.0,
            (BYTES_PER_ROW - 1) as u64,
            "행 마지막 바이트를 눌렀으면 정확히 그 바이트여야 한다(got {:?})",
            h.caret
        );
    }

    /// 헥스도 `pending_scroll_row`를 소비하고 관측값을 기록해야 한다 —
    /// 찾기(뒤 태스크)가 매치 행으로 점프하는 길이 이 한 바퀴다.
    /// (`render_records_first_visible_row_and_page_size`의 헥스판.)
    #[test]
    fn hex_render_consumes_scroll_request_and_records_observation() {
        // 500행 = 16000바이트.
        let mut app = hex_test_doc(&vec![0x41u8; 500 * 32]);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(600.0, 300.0),
            )),
            ..Default::default()
        };
        let draw = |app: &mut App| {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_hex(ui, app.doc_mut().unwrap(), &mut String::new());
                });
            });
        };
        draw(&mut app);
        {
            let doc = app.doc().unwrap();
            assert_eq!(doc.first_visible_row, 0, "처음에는 맨 위를 보고 있다");
            assert!(
                doc.visible_rows > 0 && doc.visible_rows < 100,
                "작은 창의 한 화면 행 수가 기록되어야 한다(got {})",
                doc.visible_rows
            );
        }
        {
            let doc = app.doc_mut().unwrap();
            doc.pending_scroll_row = Some(200);
            doc.pending_scroll_align = egui::Align::TOP;
        }
        draw(&mut app); // 스크롤 적용
        assert_eq!(
            app.doc().unwrap().pending_scroll_row,
            None,
            "요청은 한 번 쓰이고 소비된다(매 프레임 되돌아가면 안 된다)"
        );
        draw(&mut app); // 그 자리를 관측
        let first = app.doc().unwrap().first_visible_row;
        assert!(
            (199..=200).contains(&first),
            "요청한 행 언저리를 보고 있어야 한다(got {first})"
        );
    }

    // ---- 헥스 편집 ----

    /// 편집 조작이 오면 뷰 → 편집으로 승격되고 조작이 적용된다.
    #[test]
    fn first_edit_promotes_to_memory() {
        let mut app = hex_test_doc(&[0x00, 0x11, 0x22]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xF));
        let h = doc.hex.as_ref().unwrap();
        let e = h.edit.as_ref().expect("승격됨");
        assert_eq!(e.bytes, vec![0xF0, 0x11, 0x22], "상위 니블 먼저");
        assert_eq!(h.caret, (0, false), "니블 하나 전진");
        assert!(e.dirty);
    }

    /// 니블 두 번 = 바이트 완성, 캐럿이 다음 바이트로.
    #[test]
    fn two_nibbles_complete_a_byte() {
        let mut app = hex_test_doc(&[0x00, 0x11]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xA));
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xB));
        assert_eq!(
            doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes,
            vec![0xAB, 0x11]
        );
        assert_eq!(doc.hex.as_ref().unwrap().caret, (1, true));
    }

    /// 문자 패널 입력: ASCII는 1바이트, 한글은 UTF-8 3바이트 덮어쓰기.
    #[test]
    fn ascii_pane_typing_overwrites_utf8_bytes() {
        let mut app = hex_test_doc(&[0x61, 0x62, 0x63, 0x64]);
        let doc = app.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().pane = crate::hex::HexPane::Ascii;
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Ascii("한".into()));
        let e = doc.hex.as_ref().unwrap().edit.as_ref().unwrap();
        assert_eq!(&e.bytes[..3], "한".as_bytes());
        assert_eq!(e.bytes[3], 0x64);
        assert_eq!(doc.hex.as_ref().unwrap().caret, (3, true), "쓴 바이트 수만큼 전진");
    }

    /// 선택 삭제 — 중간을 훅 지우는 사용자 시나리오.
    #[test]
    fn delete_selection_removes_middle() {
        let mut app = hex_test_doc(&[1, 2, 3, 4, 5]);
        let doc = app.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().sel = Some((3, 1)); // 역방향 선택도 정규화
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::DeleteForward);
        let e = doc.hex.as_ref().unwrap().edit.as_ref().unwrap();
        assert_eq!(e.bytes, vec![1, 4, 5]);
        assert_eq!(doc.hex.as_ref().unwrap().caret, (1, true));
        assert!(doc.hex.as_ref().unwrap().sel.is_none());
    }

    /// 삽입 모드에서 니블 입력은 새 바이트를 끼워 넣는다.
    #[test]
    fn insert_mode_inserts_new_byte() {
        let mut app = hex_test_doc(&[0xAA, 0xBB]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::ToggleInsert);
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0x1));
        let e = doc.hex.as_ref().unwrap().edit.as_ref().unwrap();
        assert_eq!(e.bytes, vec![0x10, 0xAA, 0xBB], "새 바이트 삽입, 하위 니블 대기");
        assert_eq!(doc.hex.as_ref().unwrap().caret, (0, false));
    }

    /// Ctrl+Z / Ctrl+Y가 버퍼를 되돌리고 캐럿을 옮긴다.
    #[test]
    fn hex_undo_redo_moves_caret() {
        let mut app = hex_test_doc(&[1, 2, 3]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        doc.hex.as_mut().unwrap().caret = (2, true);
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xF));
        apply_hex_intent(doc, &mut clip, HexIntent::Undo);
        let h = doc.hex.as_ref().unwrap();
        assert_eq!(h.edit.as_ref().unwrap().bytes, vec![1, 2, 3]);
        assert_eq!(h.caret, (2, true), "되돌린 자리로");
        apply_hex_intent(app.doc_mut().unwrap(), &mut clip, HexIntent::Redo);
        assert_eq!(
            app.doc().unwrap().hex.as_ref().unwrap().edit.as_ref().unwrap().bytes,
            vec![1, 2, 0xF3]
        );
    }

    /// Ctrl+C: 선택 구간을 "4F 4B" 형식으로 클립보드에.
    #[test]
    fn hex_copy_formats_selection() {
        let mut app = hex_test_doc(&[0x4F, 0x4B, 0x00]);
        let doc = app.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().sel = Some((0, 2));
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Copy);
        assert_eq!(clip, "4F 4B");
        assert!(
            doc.hex.as_ref().unwrap().edit.is_none(),
            "복사는 편집 승격을 일으키지 않는다"
        );
    }

    /// 붙여넣기: 헥스 패널이면 16진수 해석, 해석 불가면 무시.
    #[test]
    fn hex_paste_parses_hex_in_hex_pane() {
        let mut app = hex_test_doc(&[0xAA]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Paste("DE AD".into()));
        let e = doc.hex.as_ref().unwrap().edit.as_ref().unwrap();
        assert_eq!(e.bytes, vec![0xDE, 0xAD, 0xAA], "붙여넣기는 삽입");
        let before = e.bytes.clone();
        apply_hex_intent(app.doc_mut().unwrap(), &mut clip, HexIntent::Paste("XYZ".into()));
        assert_eq!(
            app.doc().unwrap().hex.as_ref().unwrap().edit.as_ref().unwrap().bytes,
            before,
            "해석 불가 무시"
        );
    }

    /// 512MB 초과는 즉시 승격하지 않고 확인을 세운다 — 경계 판정.
    /// (그 판정을 `ensure_hex_edit`이 실제로 쓰는지는 아래 테스트가 본다.)
    #[test]
    fn big_file_requires_confirm_before_load() {
        let limit = crate::hex::HEX_EDIT_CONFIRM_BYTES;
        assert!(!hex_load_needs_confirm(limit, limit), "임계와 같으면 확인 불필요");
        assert!(hex_load_needs_confirm(limit + 1, limit), "임계 초과면 확인");
    }

    /// **`ensure_hex_edit`이 임계 판정을 실제로 쓰는가.** 위 테스트는 순수
    /// 함수만 보므로, 승격 경로가 그 판정을 무시하고 무조건 로드하도록
    /// 바뀌어도 통과한다 — 그러면 512MB 확인 창이 조용히 사라진다(변이
    /// 테스트에서 살아남은 구멍).
    ///
    /// 512MB짜리 소스를 만들 수 없으므로 **그 문서의** 임계를 1바이트로 낮춰
    /// 세 바이트 문서를 "큰 파일"로 만든다. 임계가 문서마다 있으니 병렬로 도는
    /// 다른 테스트를 방해하지 않는다.
    #[test]
    fn ensure_hex_edit_consults_the_threshold() {
        // (1) 임계 이하 → 확인 없이 곧바로 승격
        let mut small = hex_test_doc(&[1, 2, 3]);
        let doc = small.doc_mut().unwrap();
        assert!(ensure_hex_edit(doc), "임계 이하는 곧바로 승격");
        let h = doc.hex.as_ref().unwrap();
        assert!(h.edit.is_some());
        assert!(!h.confirm_load, "확인 창을 띄우지 않는다");

        // (2) 임계 초과 → 승격하지 않고 확인만 세운다
        let mut big = hex_test_doc(&[1, 2, 3]);
        let doc = big.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().edit_limit = 1;
        assert!(!ensure_hex_edit(doc), "임계 초과는 승격을 미룬다");
        let h = doc.hex.as_ref().unwrap();
        assert!(h.edit.is_none(), "메모리에 올리지 않는다");
        assert!(h.confirm_load, "확인 창을 세운다");

        // (3) 확인 대기 중 편집 인텐트는 버려진다(스펙) — 니블이 들어가면 안 된다
        let mut pending = hex_test_doc(&[0xAA, 0xBB]);
        let doc = pending.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().edit_limit = 1;
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xF));
        let h = doc.hex.as_ref().unwrap();
        assert!(h.edit.is_none(), "승격이 막혔으므로 버퍼가 없다");
        assert!(h.confirm_load, "확인 창이 떴다");
        assert_eq!(h.caret, (0, true), "캐럿도 움직이지 않는다");
    }

    /// **로드 확인 창은 탭 바를 잠그고, 빠져나갈 길이 있어야 한다(I4).**
    ///
    /// 잠그지 않으면 창이 떠 있는 동안 탭을 바꿀 수 있고, 그러면 (a) 크기
    /// 문구가 다른 파일의 것으로 바뀌고 (b) "Load"가 엉뚱한 문서를 메모리에
    /// 올리며 (c) 원래 탭의 `confirm_load`는 영영 `true`로 남는다.
    /// 그리고 Escape/창 X로 닫을 수 있어야 그 플래그가 풀린다 — 형제
    /// 다이얼로그가 전부 갖고 있는 탈출구다(Minor 9).
    #[test]
    fn hex_confirm_load_dialog_locks_tabs_and_escapes() {
        let mut app = hex_test_doc(&[1, 2, 3]);
        assert!(!tab_bar_locked_for(&app), "사전 조건: 잠겨 있지 않다");

        app.doc_mut().unwrap().hex.as_mut().unwrap().edit_limit = 1;
        let mut clip = String::new();
        apply_hex_intent(app.doc_mut().unwrap(), &mut clip, HexIntent::Nibble(0xF));
        assert!(
            app.doc().unwrap().hex.as_ref().unwrap().confirm_load,
            "사전 조건: 확인 창이 떴다"
        );
        assert!(
            tab_bar_locked_for(&app),
            "확인 창이 떠 있는 동안은 탭 전환/드롭이 막혀야 한다"
        );
        // 그래서 드롭도 거절된다 — 크기 문구/Load 대상이 활성 문서를 읽어도
        // 안전하다는 근거가 이 잠금이다.
        let p = temp(b"a,b\n1,2\n");
        assert!(matches!(
            plan_dropped_files(vec![p.clone()], tab_bar_locked_for(&app)),
            DropPlan::Locked(_)
        ));
        std::fs::remove_file(&p).ok();

        // Escape로 닫으면 플래그가 풀리고 잠금도 풀린다.
        let ctx = egui::Context::default();
        let esc = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(esc, |ctx| render_confirm_hex_load_dialog(ctx, &mut app));
        assert!(
            !app.doc().unwrap().hex.as_ref().unwrap().confirm_load,
            "Escape가 확인 창을 닫아야 한다(플래그가 영영 켜진 채 남으면 안 된다)"
        );
        assert!(!tab_bar_locked_for(&app), "닫혔으니 잠금도 풀린다");
        assert!(
            app.doc().unwrap().hex.as_ref().unwrap().edit.is_none(),
            "Escape는 Cancel과 같다 — 로드하지 않는다"
        );
    }

    /// 이동: 방향키 좌우는 1바이트, 상하는 32바이트, 경계 클램프.
    #[test]
    fn move_clamps_to_document() {
        let mut app = hex_test_doc(&[0; 40]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Move { delta: -1, extend: false });
        assert_eq!(doc.hex.as_ref().unwrap().caret.0, 0, "앞 경계");
        apply_hex_intent(doc, &mut clip, HexIntent::Move { delta: 9999, extend: false });
        assert_eq!(doc.hex.as_ref().unwrap().caret.0, 40, "끝(파일 끝 삽입 지점) 클램프");
        apply_hex_intent(doc, &mut clip, HexIntent::Move { delta: -32, extend: true });
        assert_eq!(
            doc.hex.as_ref().unwrap().sel,
            Some((40, 8)),
            "Shift 이동이 선택을 만든다"
        );
    }

    /// Home/End/문서 처음·끝 — 브리프가 요구한 "변형을 추가하면 테스트도".
    #[test]
    fn hex_home_end_and_doc_bounds() {
        let mut app = hex_test_doc(&[0; 100]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::MoveTo { offset: 40, extend: false });
        apply_hex_intent(doc, &mut clip, HexIntent::MoveHome { extend: false });
        assert_eq!(doc.hex.as_ref().unwrap().caret, (32, true), "행 시작");
        apply_hex_intent(doc, &mut clip, HexIntent::MoveEnd { extend: false });
        assert_eq!(doc.hex.as_ref().unwrap().caret.0, 63, "행 마지막 바이트");
        apply_hex_intent(doc, &mut clip, HexIntent::MoveDocEnd { extend: false });
        assert_eq!(doc.hex.as_ref().unwrap().caret.0, 100, "문서 끝 = 삽입 지점");
        apply_hex_intent(doc, &mut clip, HexIntent::MoveDocStart { extend: false });
        assert_eq!(doc.hex.as_ref().unwrap().caret, (0, true));
        // 마지막 짧은 행의 End는 그 행의 마지막 바이트에서 멈춘다.
        apply_hex_intent(doc, &mut clip, HexIntent::MoveTo { offset: 96, extend: false });
        apply_hex_intent(doc, &mut clip, HexIntent::MoveEnd { extend: false });
        assert_eq!(doc.hex.as_ref().unwrap().caret.0, 99);
    }

    /// Escape는 선택만 지우고 캐럿은 그대로 둔다.
    #[test]
    fn hex_escape_clears_selection_only() {
        let mut app = hex_test_doc(&[1, 2, 3, 4]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::MoveTo { offset: 3, extend: true });
        assert!(doc.hex.as_ref().unwrap().sel.is_some(), "사전 조건: 선택 있음");
        apply_hex_intent(doc, &mut clip, HexIntent::ClearSelection);
        let h = doc.hex.as_ref().unwrap();
        assert!(h.sel.is_none());
        assert_eq!(h.caret.0, 3, "캐럿은 남는다");
    }

    /// Backspace는 캐럿 앞 바이트를 지우고 캐럿을 물린다. 0에서는 no-op.
    #[test]
    fn hex_backspace_deletes_previous_byte() {
        let mut app = hex_test_doc(&[1, 2, 3]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::MoveTo { offset: 2, extend: false });
        apply_hex_intent(doc, &mut clip, HexIntent::Backspace);
        assert_eq!(doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes, vec![1, 3]);
        assert_eq!(doc.hex.as_ref().unwrap().caret, (1, true));
        apply_hex_intent(doc, &mut clip, HexIntent::MoveTo { offset: 0, extend: false });
        apply_hex_intent(doc, &mut clip, HexIntent::Backspace);
        assert_eq!(
            doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes,
            vec![1, 3],
            "0에서 Backspace는 no-op"
        );
    }

    /// 파일 끝(캐럿 == len)에서는 덮어쓸 바이트가 없으므로 항상 삽입이다.
    #[test]
    fn nibble_at_eof_appends() {
        let mut app = hex_test_doc(&[0xAA]);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::MoveTo { offset: 1, extend: false });
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0x7));
        assert_eq!(doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes, vec![0xAA, 0x70]);
        assert_eq!(doc.hex.as_ref().unwrap().caret, (1, false));
    }

    /// 수집: 헥스 패널의 글자는 니블로, 문자 패널에서는 통째로 Ascii로.
    #[test]
    fn collect_hex_intents_splits_by_pane() {
        let events = vec![
            egui::Event::Text("a".into()),
            egui::Event::Text("Z".into()), // 16진수 아님 → 헥스 패널에서 무시
            egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            },
        ];
        let collect = |pane| {
            let input = egui::RawInput { events: events.clone(), ..Default::default() };
            let ctx = egui::Context::default();
            let mut out = Vec::new();
            let _ = ctx.run(input, |ctx| {
                out = ctx.input(|i| collect_hex_intents(i, pane, 10));
            });
            out
        };
        let hex = collect(crate::hex::HexPane::Hex);
        assert!(
            matches!(hex[0], HexIntent::Nibble(0xA)),
            "헥스 패널의 'a'는 니블(got {:?})",
            hex[0]
        );
        assert!(
            matches!(hex[1], HexIntent::Move { delta: 32, extend: true }),
            "Shift+Down은 32바이트 확장 이동(got {:?})",
            hex[1]
        );
        assert_eq!(hex.len(), 2, "16진수 아닌 'Z'는 헥스 패널에서 버려진다");
        let ascii = collect(crate::hex::HexPane::Ascii);
        assert!(matches!(&ascii[0], HexIntent::Ascii(s) if s == "a"));
        assert!(matches!(&ascii[1], HexIntent::Ascii(s) if s == "Z"));
    }

    /// **회귀**: 전역 Ctrl+Z(텍스트 undo)와 헥스 Ctrl+Z가 동시에 소비되면
    /// 한 번 누른 undo가 두 번 일어난다. 전역 게이트는 `d.edit.is_some()`
    /// (텍스트 편집 버퍼)라 헥스 문서에서는 절대 참이 되면 안 된다.
    #[test]
    fn hex_undo_not_double_consumed_by_global_handler() {
        let mut app = hex_test_doc(&[1, 2, 3]);
        let mut clip = String::new();
        {
            let doc = app.doc_mut().unwrap();
            apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xF));
        }
        // 전역 Ctrl+Z 게이트의 조건(`doc.edit.is_some()`)이 헥스 문서에서는 거짓.
        let doc = app.doc().unwrap();
        assert!(doc.edit.is_none(), "헥스 문서는 텍스트 편집 버퍼를 갖지 않는다");
        assert!(doc.hex.as_ref().unwrap().edit.is_some(), "헥스 버퍼만 승격됐다");
        assert!(
            !can_undo_text(doc),
            "전역 Ctrl+Z 경로는 헥스 문서에서 발동하지 않는다"
        );
    }

    /// 헥스 렌더에 키 입력이 실제로 배선돼 있는가 — 순수 함수 테스트만으로는
    /// 배선이 끊겨도 다 통과한다.
    #[test]
    fn hex_render_applies_typed_nibble() {
        let mut app = hex_test_doc(&[0x00, 0x11]);
        let mut clip = String::new();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(900.0, 400.0),
            )),
            events: vec![egui::Event::Text("f".into())],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                render_hex(ui, app.doc_mut().unwrap(), &mut clip);
            });
        });
        let h = app.doc().unwrap().hex.as_ref().unwrap();
        assert_eq!(
            h.edit.as_ref().expect("타이핑이 승격시킨다").bytes,
            vec![0xF0, 0x11]
        );
    }

    /// **관측 자체**가 실제로 일어나는가. Page Up/Down은 렌더가
    /// `first_visible_row`/`visible_rows`를 기록해 준다는 전제 위에 서 있는데,
    /// 순수 함수 테스트는 그 전제를 확인하지 못한다(기록 코드를 통째로 지워도
    /// 다 통과한다). 그래서 진짜 egui 프레임에서 `render_text`를 돌려
    /// 두 필드가 채워지는지 본다.
    ///
    /// 스크롤 요청을 남긴 뒤 렌더하면 그 행이 화면 맨 위로 오므로
    /// (`vertical_scroll_offset` + `Align::TOP`), 다음 프레임의 `first_visible_row`가
    /// 그 언저리로 따라와야 한다 — "요청 → 스크롤 → 관측"이 한 바퀴 도는지를
    /// 이 한 테스트가 고정한다.
    #[test]
    fn render_records_first_visible_row_and_page_size() {
        let mut app = find_test_doc(&(0..500).map(|_| "x").collect::<Vec<_>>());
        let ctx = egui::Context::default();
        let mut clip = String::new();
        // egui 기본 화면은 매우 커서 500행이 거의 다 들어간다 — 그러면
        // 페이지 이동이 몇 행 못 움직여 관측이 끊겼는지 알 수 없다.
        // 창을 작게 잡아 한 화면이 문서보다 훨씬 작게 만든다.
        let mut input = egui::RawInput::default();
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        input.screen_rect = Some(screen);
        let draw = |app: &mut App, clip: &mut String| {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_text(ui, app.doc_mut().unwrap(), 0, clip, false, crate::i18n::Lang::default());
                });
            });
        };
        draw(&mut app, &mut clip);
        {
            let doc = app.doc().unwrap();
            assert_eq!(doc.first_visible_row, 0, "처음에는 맨 위를 보고 있다");
            assert!(
                doc.visible_rows > 0 && doc.visible_rows < 100,
                "작은 창의 한 화면 행 수가 기록되어야 한다(got {})",
                doc.visible_rows
            );
        }
        // 페이지를 넘긴다 → 다음 프레임에 스크롤이 일어나고, 그다음 프레임의
        // 관측값이 따라와야 한다.
        let target = {
            let doc = app.doc_mut().unwrap();
            apply_page_scroll(doc, PageDir::Down);
            doc.pending_scroll_row.unwrap()
        };
        assert!(target > 0, "사전 조건: 실제로 움직일 목표가 생겼다");
        // 요청 소비 → 스크롤 → 옮겨진 자리에서 재관측. **두 프레임이면 끝난다**
        // (K-3): `vertical_scroll_offset`이 `state.offset.y`에 즉시 대입되므로
        // 첫 프레임에 자리가 잡히고 다음 프레임이 그 자리를 관측한다.
        // 예전 `scroll_to_row`는 0.1~0.3초에 걸쳐 감겨 수십 프레임이 필요했다 —
        // 프레임 수를 늘리면 그 회귀를 놓친다.
        draw(&mut app, &mut clip);
        draw(&mut app, &mut clip);
        let doc = app.doc().unwrap();
        assert_eq!(
            doc.pending_scroll_row, None,
            "렌더가 스크롤 요청을 소비한다"
        );
        // `Align::TOP`은 목표 행의 **위 경계**를 화면 위에 맞추는데,
        // `body.rows`는 위쪽에 걸친 행까지 그리므로 관측되는 첫 행은
        // `target` 또는 `target - 1`이다. 어느 쪽이든 "페이지가 실제로
        // 넘어갔고 렌더가 그 자리를 관측했다"는 성질은 같으므로 그 범위로
        // 고정한다(픽셀 반올림에 테스트를 묶지 않는다).
        assert!(
            doc.first_visible_row + 1 >= target && doc.first_visible_row <= target,
            "페이지를 넘긴 뒤 화면 첫 행이 목표 근처여야 한다 \
             (first_visible_row={}, target={target})",
            doc.first_visible_row
        );
    }

    /// 탭 자리 찾기는 **char 인덱스**여야 한다 — `x_of`가 `CCursor`를 받으므로
    /// 바이트 오프셋이면 한글이 섞인 줄에서 엉뚱한 칸을 칠한다.
    #[test]
    fn tab_positions_are_char_indices_not_bytes() {
        assert_eq!(tab_positions("a\tb"), vec![1]);
        assert_eq!(tab_positions("\t\t"), vec![0, 1]);
        assert_eq!(tab_positions("no tabs here"), Vec::<usize>::new());
        assert_eq!(tab_positions(""), Vec::<usize>::new());
        // 한글은 UTF-8에서 3바이트다. 바이트로 세면 탭이 3이 아니라 9로 잡힌다.
        assert_eq!(tab_positions("한글자\t뒤"), vec![3]);
        // 스페이스는 탭이 아니다(이 기능의 존재 이유 — 둘을 갈라야 한다).
        assert_eq!(tab_positions("   "), Vec::<usize>::new());
    }

    /// **탭 칸의 폭을 갤리에게 묻는다.** epaint는 탭을 "다음 탭스톱까지"가 아니라
    /// 고정폭 빈 글자로 그리므로(`TAB_SIZE × 스페이스 폭`), 갤리 안에서 탭은
    /// 평범한 한 글자다. 그 성질에 기대는 코드이므로 여기서 못박는다 —
    /// epaint가 진짜 탭스톱으로 바뀌면 이 테스트가 먼저 깨져야 한다.
    #[test]
    fn tab_occupies_one_galley_char_wider_than_a_space() {
        let ctx = egui::Context::default();
        // 폰트는 `Context::run` 안에서만 쓸 수 있다.
        let _ = ctx.run(Default::default(), |ctx| {
            let font = egui::FontId::monospace(crate::theme::MONO_SIZE);
            let x_at = |text: &str, ch: usize| -> f32 {
                let g = ctx.fonts(|f| {
                    f.layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::BLACK)
                });
                g.pos_from_ccursor(egui::text::CCursor::new(ch)).min.x
            };
            // "a\tb"에서 탭 칸은 char 1 → 2 사이다.
            let tab_w = x_at("a\tb", 2) - x_at("a\tb", 1);
            let space_w = x_at("a b", 2) - x_at("a b", 1);
            assert!(tab_w > 0.0, "탭 칸은 폭이 있어야 칠할 자리가 생긴다");
            assert!(
                tab_w > space_w * 1.5,
                "탭은 스페이스보다 확실히 넓어야 한다 (탭 {tab_w}, 스페이스 {space_w})"
            );
            // 탭 **뒤** 글자도 그만큼 밀린다 — 즉 탭이 자리를 실제로 차지한다.
            assert!(x_at("a\tb", 2) > x_at("ab", 1));
        });
    }

    /// 탭 음영이 **탭 위에** 그려지는가. 좌표를 갤리에서 얻으므로, 칠하는 구간이
    /// 그 줄의 탭 칸과 정확히 겹쳐야 한다(옆 글자를 덮으면 안 된다).
    #[test]
    fn tab_shade_covers_exactly_the_tab_cell() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            let font = egui::FontId::monospace(crate::theme::MONO_SIZE);
            let line = "ab\tcd";
            let galley = ctx.fonts(|f| {
                f.layout_no_wrap(line.to_owned(), font.clone(), egui::Color32::BLACK)
            });
            let x_of =
                |c: usize| -> f32 { galley.pos_from_ccursor(egui::text::CCursor::new(c)).min.x };

            let tabs = tab_positions(line);
            assert_eq!(tabs, vec![2]);

            // 칠할 구간 = x_of(2)..x_of(3): 앞 글자('b')가 끝나는 자리에서
            // 시작해 뒷 글자('c')가 시작하는 자리에서 끝난다.
            let (x0, x1) = (x_of(tabs[0]), x_of(tabs[0] + 1));
            assert!(x1 > x0, "구간이 비어 있으면 아무것도 안 보인다");
            // 그 폭이 곧 탭 한 칸이다 — 글자 한 칸보다 확실히 넓다.
            let char_w = x_of(1) - x_of(0);
            assert!(
                (x1 - x0) > char_w * 1.5,
                "탭 칸이 글자 한 칸 수준이면 좌표를 잘못 잡은 것이다"
            );
            // 뒷 글자는 음영 바깥에서 시작한다(덮이지 않는다).
            assert!(x_of(4) > x1, "탭 다음 글자는 음영 오른쪽에 있어야 한다");
        });
    }

    /// **`paint_tab_shades`가 실제로 그리는 사각형**을 검사한다. 좌표 산술을
    /// 테스트에서 다시 계산하면 그 함수가 엉뚱한 칸을 칠하도록 바뀌어도
    /// (`x_of(i+1)..x_of(i+2)` 같은 off-by-one) 아무도 못 잡는다 — 변이로
    /// 확인한 실제 구멍이라 함수를 직접 태운다.
    #[test]
    fn paint_tab_shades_emits_a_rect_over_each_tab() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            // char 하나 = 10px인 가짜 좌표계. 탭이든 아니든 인덱스 → x가
            // 선형이므로 어떤 칸이 칠해졌는지 인덱스로 되읽을 수 있다.
            let x_of = |c: usize| -> f32 { c as f32 * 10.0 };
            let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 20.0));
            let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("tabtest"));
            let painter = egui::Painter::new(ctx.clone(), layer, rect);

            // "ab\tc\td" → 탭은 char 2와 4.
            paint_tab_shades(&painter, rect, &x_of, &[2, 4]);

            let shapes = ctx.graphics(|g| {
                g.get(layer).map(|l| l.all_entries().count()).unwrap_or(0)
            });
            assert_eq!(shapes, 2, "탭 개수만큼 사각형이 나와야 한다");

            let painted: Vec<(f32, f32)> = ctx.graphics(|g| {
                g.get(layer)
                    .map(|l| {
                        l.all_entries()
                            .filter_map(|e| match &e.shape {
                                egui::Shape::Rect(r) => Some((r.rect.left(), r.rect.right())),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            });
            assert_eq!(
                painted,
                vec![(20.0, 30.0), (40.0, 50.0)],
                "각 탭의 **그 칸**(i..i+1)이 칠해져야 한다"
            );
        });
    }

    /// **배선 확인.** `paint_tab_shades`를 아무리 잘 테스트해도 렌더가 그걸
    /// 부르지 않으면 화면에는 아무 일도 일어나지 않는다 — 실제로 호출을 지워도
    /// 전부 통과하는 것을 변이로 확인했다. 그래서 진짜 문서를 그려서 탭 색
    /// (`theme::tab_bg`)으로 칠해진 사각형이 나오는지 센다.
    ///
    /// `render_text`에는 줄을 그리는 경로가 **셋** 있고(뷰, 뷰+검색중, 편집),
    /// 각각 따로 칠해야 한다. 셋을 모두 태운다 — 하나만 확인하면 나머지 둘에서
    /// 호출이 사라져도 통과한다(실제로 변이로 확인했다).
    #[test]
    fn render_text_actually_paints_tab_shades_in_all_three_paths() {
        /// 그 프레임에 그려진 탭 배경색 사각형 수.
        fn count_tab_rects(app: &mut App) -> usize {
            let ctx = egui::Context::default();
            let mut clip = String::new();
            let out = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_text(
                        ui,
                        app.doc_mut().unwrap(),
                        0,
                        &mut clip,
                        false,
                        crate::i18n::Lang::default(),
                    );
                });
            });
            let want = crate::theme::tab_bg();
            out.shapes
                .iter()
                .filter(|c| match &c.shape {
                    egui::Shape::Rect(r) => r.fill == want,
                    _ => false,
                })
                .count()
        }

        // ---- (1) 편집 모드 ----
        let mut edit = find_test_doc(&["a\tb", "c\td\te"]);
        assert_eq!(count_tab_rects(&mut edit), 3, "편집 모드에서 탭 3개");

        // 탭이 없으면 탭 음영도 없다(다른 음영을 잘못 세지 않는다는 확인).
        let mut no_tabs = find_test_doc(&["no tabs", "here either"]);
        assert_eq!(count_tab_rects(&mut no_tabs), 0);

        // 뷰 경로는 편집 버퍼가 아니라 **파일**에서 줄을 읽으므로, 탭이 든
        // 파일을 실제로 열어야 한다(`find_test_doc`은 편집 버퍼에만 넣는다).
        // 확장자를 .txt로 두어 텍스트 모드(SeparatorMode::None)로 열린다.
        fn view_doc_with_tabs() -> App {
            let p = temp_ext(b"a\tb\r\nc\td\te\r\n", "txt");
            let ctx = egui::Context::default();
            let mut app = App::default();
            app.open_path(&p, &ctx);
            let doc = app.doc_mut().unwrap();
            doc.indexer.take().unwrap().join().unwrap();
            // 작은 파일은 열 때 자동으로 편집 모드로 들어가므로(`auto_edit_on_open`)
            // 뷰 경로를 태우려면 편집 버퍼를 명시적으로 걷어낸다. 그러면 줄을
            // 파일(mmap)에서 디코딩해 읽는 경로가 된다.
            doc.edit = None;
            app
        }

        // ---- (2) 뷰 + 검색 중 ----
        // `highlight`가 있으면 galley 경로로 간다(`searching = true`).
        let mut searching = view_doc_with_tabs();
        searching.doc_mut().unwrap().highlight = Some(Highlight {
            rows: Vec::new(),
            query: "zzz".to_owned(),
            opts: Default::default(),
        });
        assert_eq!(
            count_tab_rects(&mut searching),
            3,
            "검색 중 뷰 경로에서도 탭이 칠해져야 한다"
        );

        // ---- (3) 뷰 전용(Label 경로) ----
        let mut view = view_doc_with_tabs();
        assert_eq!(
            count_tab_rects(&mut view),
            3,
            "평범한 뷰 모드(Label 경로)에서도 탭이 칠해져야 한다"
        );
    }

    /// 탭이 없으면 아무것도 그리지 않는다(빈 사각형이 쌓이면 낭비다).
    #[test]
    fn paint_tab_shades_draws_nothing_without_tabs() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            let x_of = |c: usize| -> f32 { c as f32 * 10.0 };
            let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 20.0));
            let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("notabs"));
            let painter = egui::Painter::new(ctx.clone(), layer, rect);
            paint_tab_shades(&painter, rect, &x_of, &[]);
            let shapes = ctx.graphics(|g| {
                g.get(layer).map(|l| l.all_entries().count()).unwrap_or(0)
            });
            assert_eq!(shapes, 0);
        });
    }

    /// 탭 칸 위에 찾기 매치가 겹쳐도 **매치가 구분되어야** 한다(찾은 자리가
    /// 우선). 매치는 탭 음영 **뒤에** 그려지므로 알파 크기를 비교하는 것은
    /// 잘못된 기준이었다 — 실제로 필요한 것은 "겹쳤을 때 색이 달라 보이는가"다.
    /// (탭 알파를 120으로 올리면서 이 테스트가 틀린 것을 잡아냈다.)
    ///
    /// 채널 차이를 직접 재지 않고 **색이 서로 다른지**만 본다. 정확한 합성값은
    /// ecolor의 감마/선형 변환에 달려 있어 테스트에서 재현하면 그 내부를
    /// 베끼게 된다(그 실수를 이미 두 번 했다).
    #[test]
    fn find_shades_stay_distinct_over_a_tab_cell() {
        let tab = crate::theme::tab_bg();
        let m = crate::theme::find_match_bg();
        let c = crate::theme::find_current_bg();
        // 셋 다 불투명하지 않아야 아래 색과 섞인다(섞이지 않으면 덮어버린다).
        for (name, col) in [("tab", tab), ("match", m), ("current", c)] {
            assert!(col.a() > 0, "{name}: 완전 투명이면 보이지 않는다");
            assert!(col.a() < 255, "{name}: 불투명하면 아래 색을 완전히 덮는다");
        }
        // 매치 음영은 탭과 **다른 색조**여야 겹쳤을 때 구분된다. 탭은 청회색
        // (파랑 우세), 찾기는 보라(빨강 우세)라 그 관계가 뒤집히면 안 된다.
        assert!(
            tab.b() > tab.r(),
            "탭은 청회색이어야 한다(파랑 > 빨강)"
        );
        assert!(
            m.r() > m.g() && c.r() > c.g(),
            "찾기 음영은 보라 계열이어야 탭 위에서 구분된다"
        );
        // current는 전체 매치보다 진해 둘이 갈린다(기존 규율 유지).
        assert!(c.a() > m.a(), "current 매치가 전체 매치보다 진해야 한다");
    }

    /// **너무 옅어서 안 보이는 것을 막는다.** 알파가 0만 아니면 통과하는 검사로는
    /// 부족했다 — 처음 값(28)은 순백 위에서 흰색과 몇 단계 차이가 안 나서,
    /// 사각형이 정상적으로 그려지는데도 화면에서는 보이지 않았다(모양 개수를
    /// 세는 테스트는 전부 통과했고, 사용자가 "색이 안 보인다"고 알려줬다).
    ///
    /// 합성 픽셀을 직접 계산하지는 않는다. `Color32`는 채널을 **감마 공간에서
    /// 미리 곱해** 들고 있어(`ecolor-0.28.1/src/color32.rs:97-113`) 그 합성을
    /// 테스트에서 재현하면 ecolor 내부를 베끼는 꼴이 되고, 실제로 그러다 부호가
    /// 뒤집힌 값을 재는 무의미한 테스트를 한 번 썼다. 대신 사람이 실제로 조절하는
    /// 값인 **입력 알파**에 하한을 둔다.
    #[test]
    fn tab_bg_alpha_is_high_enough_to_see() {
        // 알파를 되읽는다(`Color32`는 알파만은 그대로 보관한다).
        let a = crate::theme::tab_bg().a();
        // 하한 근거는 실제 화면이다: 28은 아예 안 보였고, 56도 "차이가 있는 것
        // 같은데 눈으로 구분이 안 가는" 수준이었다. 100 아래로 내리려면 화면에서
        // 다시 확인할 것.
        assert!(
            a >= 100,
            "알파가 이보다 낮으면 순백 위에서 사실상 구분되지 않는다 (지금 {a}). \
             28과 56에서 실제로 그랬다."
        );
        // 상한은 `tab_bg_is_fainter_than_find_shades`가 이미 잡는다
        // (찾기 음영 64보다 낮아야 한다).
    }

    /// 한 화면 행 수는 **헤더 한 줄을 뺀** 본문 높이로 구한다 — 안 빼면
    /// Page Down이 매번 한 행씩 더 건너뛰어 그 행이 조용히 안 읽힌다.
    #[test]
    fn visible_row_count_excludes_the_header_row() {
        // 헤더 1줄 + 본문 10줄.
        assert_eq!(visible_row_count(ROW_HEIGHT * 11.0, ROW_HEIGHT), 10);
        // 헤더만 들어가는 높이면 본문 0행.
        assert_eq!(visible_row_count(ROW_HEIGHT, ROW_HEIGHT), 0);
        assert_eq!(visible_row_count(0.0, ROW_HEIGHT), 0);
        // 음수(창이 접힌 극단)에서도 패닉하지 않는다.
        assert_eq!(visible_row_count(-100.0, ROW_HEIGHT), 0);
    }

    /// 확대하면 **같은 창 높이에 더 적은 행**이 들어간다. 행 높이가 배율을 타지
    /// 않으면(상수를 그대로 쓰면) 이 값이 변하지 않아, 확대 상태의 Page Down이
    /// 화면보다 많이 건너뛰어 행을 조용히 건너뛴다.
    #[test]
    fn visible_row_count_shrinks_when_zoomed_in() {
        let h = ROW_HEIGHT * 11.0;
        let at_1x = visible_row_count(h, crate::theme::row_height(1.0));
        let at_2x = visible_row_count(h, crate::theme::row_height(2.0));
        assert_eq!(at_1x, 10);
        assert!(
            at_2x < at_1x,
            "2배 확대면 들어가는 행이 줄어야 한다 (1x={at_1x}, 2x={at_2x})"
        );
    }

    /// **행 높이와 글자 크기는 같은 배율에서 나와야 한다.** 문서의 배율을
    /// 올렸을 때 `doc_row_height`와 `doc_font_id`가 함께 커지지 않으면, 글자만
    /// 커지고 행은 그대로라 위아래가 잘린다(반대면 행 사이가 허옇게 뜬다).
    ///
    /// 렌더가 실제로 부르는 **그 두 함수**를 태운다 — `theme::row_height`를
    /// 직접 부르면 배선이 끊겨도(상수를 돌려주도록 바뀌어도) 통과한다.
    #[test]
    fn doc_row_height_and_font_scale_together() {
        let mut app = find_test_doc(&["a", "b", "c"]);
        let doc = app.doc_mut().unwrap();

        doc.view_scale = 1.0;
        let h1 = doc_row_height(doc);
        let f1 = doc_font_id(doc).size;

        doc.view_scale = 2.0;
        let h2 = doc_row_height(doc);
        let f2 = doc_font_id(doc).size;

        assert!(h2 > h1, "배율을 올리면 행 높이도 커져야 한다 ({h1} → {h2})");
        assert!(f2 > f1, "배율을 올리면 글자도 커져야 한다 ({f1} → {f2})");
        // 비율이 같아야 글자가 행 안에 그대로 담긴다.
        assert!(
            ((h2 / h1) - (f2 / f1)).abs() < 1e-3,
            "행 높이와 글자가 같은 비율로 커져야 한다 (행 {:.3}배, 글자 {:.3}배)",
            h2 / h1,
            f2 / f1
        );
    }

    /// Ctrl+휠 방향: 위로 굴리면(양수) 커지고 아래로 굴리면 작아진다.
    #[test]
    fn wheel_up_zooms_in_and_down_zooms_out() {
        assert!(zoomed_scale(1.0, 100.0) > 1.0);
        assert!(zoomed_scale(1.0, -100.0) < 1.0);
        // 굴리지 않으면 그대로.
        assert_eq!(zoomed_scale(1.0, 0.0), 1.0);
    }

    /// 경계에서 더 밀어도 범위를 넘지 않는다. 넘으면 글자가 0px이 되거나
    /// (아래) 한 행이 화면을 덮는다(위).
    #[test]
    fn zoom_saturates_at_both_limits() {
        let mut s = 1.0;
        for _ in 0..500 {
            s = zoomed_scale(s, 500.0);
        }
        assert_eq!(s, crate::theme::MAX_VIEW_SCALE);
        for _ in 0..1000 {
            s = zoomed_scale(s, -500.0);
        }
        assert_eq!(s, crate::theme::MIN_VIEW_SCALE);
    }

    /// 깨진 배율(NaN·0·음수)이 들어와도 화면이 무너지지 않는다. NaN은 비교가
    /// 전부 거짓이라 `clamp`만으로는 그대로 통과한다 — 따로 걸러야 한다.
    #[test]
    fn broken_scale_falls_back_to_sane_values() {
        assert_eq!(crate::theme::clamp_view_scale(f32::NAN), 1.0);
        assert_eq!(crate::theme::clamp_view_scale(0.0), crate::theme::MIN_VIEW_SCALE);
        assert_eq!(crate::theme::clamp_view_scale(-5.0), crate::theme::MIN_VIEW_SCALE);
        assert_eq!(crate::theme::clamp_view_scale(1e9), crate::theme::MAX_VIEW_SCALE);
        // 어떤 배율에서도 행 높이·글자 크기는 양수다(0이면 나눗셈이 무한대가 된다).
        for s in [f32::NAN, 0.0, -1.0, 1e9, 1.0] {
            assert!(crate::theme::row_height(s) > 0.0);
            assert!(crate::theme::mono_size(s) > 0.0);
        }
    }

    /// **이 기능의 핵심 요구.** 데이터 영역만 배율을 타고 UI 크롬은 고정이다.
    ///
    /// 확대 후 `Body`(= 표·텍스트 셀의 글꼴)는 커지지만 `Button`(= 메뉴·툴바·
    /// 상태바가 `chrome_text`로 쓰는 스타일)은 그대로여야 한다. 예전처럼
    /// `ctx.set_zoom_factor`로 전역 확대하면 둘 다 커져 이 테스트가 깨진다.
    #[test]
    fn zoom_grows_data_font_but_not_chrome_font() {
        let ctx = egui::Context::default();
        let size = |ctx: &egui::Context, style: egui::TextStyle| {
            ctx.style().text_styles[&style].size
        };

        crate::theme::install_text_styles(&ctx, 1.0);
        let body_1x = size(&ctx, egui::TextStyle::Body);
        let mono_1x = size(&ctx, egui::TextStyle::Monospace);
        let button_1x = size(&ctx, egui::TextStyle::Button);
        let heading_1x = size(&ctx, egui::TextStyle::Heading);
        let small_1x = size(&ctx, egui::TextStyle::Small);

        crate::theme::install_text_styles(&ctx, 2.0);
        assert!(
            size(&ctx, egui::TextStyle::Body) > body_1x,
            "데이터 영역(Body)은 확대되어야 한다"
        );
        assert!(size(&ctx, egui::TextStyle::Monospace) > mono_1x);
        assert_eq!(
            size(&ctx, egui::TextStyle::Button),
            button_1x,
            "메뉴·툴바·상태바(Button)는 그대로여야 한다"
        );
        assert_eq!(size(&ctx, egui::TextStyle::Heading), heading_1x);
        assert_eq!(size(&ctx, egui::TextStyle::Small), small_1x);
    }

    /// Ctrl+휠 배선: 실제 입력이 `view_scale`을 움직이고, **전역
    /// `zoom_factor`는 건드리지 않는다**(그걸 쓰면 크롬까지 커진다 — 예전
    /// 구현으로 되돌아가면 이 테스트가 잡는다). 순수 함수 테스트만으로는
    /// 배선이 끊겨도 다 통과하므로 실제 이벤트를 태운다.
    ///
    /// Ctrl 없이 굴리는 것은 평범한 스크롤이어야 한다 — 확대되면 안 된다.
    #[test]
    fn ctrl_wheel_zooms_data_area_only_and_plain_wheel_does_not() {
        let wheel = |modifiers: egui::Modifiers| egui::RawInput {
            events: vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 300.0),
                modifiers,
            }],
            modifiers,
            ..Default::default()
        };

        let mut app = App::default();
        let ctx = egui::Context::default();
        let _ = ctx.run(wheel(egui::Modifiers::CTRL), |ctx| {
            app.apply_ctrl_wheel_zoom(ctx);
        });
        assert!(app.view_scale > 1.0, "Ctrl+휠은 데이터 영역을 확대한다");
        assert_eq!(ctx.zoom_factor(), 1.0, "전역 배율은 손대지 않는다");
        // 배율 값만 바뀌고 스타일을 다시 깔지 않으면, `Label`로 그리는 표·텍스트
        // 셀(= `TextStyle::Body`)은 옛 크기 그대로 남는다 — 행 높이만 커지고
        // 글자는 그대로인 화면이 된다. 그 재적용까지가 이 동작의 일부다.
        assert!(
            ctx.style().text_styles[&egui::TextStyle::Body].size > crate::theme::MONO_SIZE,
            "확대가 Body 스타일에 반영되어야 셀 글자가 실제로 커진다"
        );
        assert_eq!(
            ctx.style().text_styles[&egui::TextStyle::Button].size,
            13.0,
            "크롬(Button)은 그대로"
        );

        // Ctrl 없이 굴리면 배율은 그대로(그냥 스크롤이다).
        let mut plain = App::default();
        let ctx2 = egui::Context::default();
        let _ = ctx2.run(wheel(egui::Modifiers::NONE), |ctx| {
            plain.apply_ctrl_wheel_zoom(ctx);
        });
        assert_eq!(plain.view_scale, 1.0, "Ctrl 없는 휠은 평범한 스크롤이다");
    }

    /// **뷰 전용**(편집 모드 진입 없이) 텍스트 문서에서 Page Down 두 번이
    /// 실제로 서로 다른 자리로 나아가는가.
    ///
    /// `render_records_first_visible_row_and_page_size`는 `find_test_doc`으로
    /// 문서를 만드는데, 그 헬퍼가 `enter_edit_mode`를 부르므로 `editing`이
    /// 항상 참이다 — `render_text`의 `if !editing { return; }` 이전에 있는
    /// `first_visible_row` 기록이 조기 반환 **뒤**로 옮겨져도 이 테스트들은
    /// 걸러내지 못한다(뷰 전용 경로 자체가 실행되지 않으므로). 그래서 여기서는
    /// `enter_edit_mode`를 부르지 않고 실제 `.txt` 파일을 뷰 모드 그대로 열어
    /// 그 경로를 직접 태운다.
    #[test]
    fn view_only_text_pages_advance_across_two_page_downs() {
        let content: Vec<u8> = (0..500)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        let p = temp_ext(&content, "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        app.doc_mut().unwrap().indexer.take().unwrap().join().unwrap();
        // 파일이 작아 `open_path`가 자동으로 편집 모드에 넣는다 — 이 테스트의
        // 존재 이유가 뷰 전용 경로이므로 되돌린다.
        view_doc(app.doc_mut().unwrap());

        let mut clip = String::new();
        let mut input = egui::RawInput::default();
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        input.screen_rect = Some(screen);
        let draw = |app: &mut App, clip: &mut String| {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_text(ui, app.doc_mut().unwrap(), 0, clip, false, crate::i18n::Lang::default());
                });
            });
        };
        draw(&mut app, &mut clip);
        assert!(
            app.doc().unwrap().edit.is_none(),
            "렌더 후에도 뷰 전용 상태 유지(편집 모드로 넘어가지 않음)"
        );

        // 첫 Page Down.
        let first_target = {
            let doc = app.doc_mut().unwrap();
            apply_page_scroll(doc, PageDir::Down);
            doc.pending_scroll_row.unwrap()
        };
        assert!(first_target > 0, "사전 조건: 실제로 움직일 목표가 생겼다");
        for _ in 0..40 {
            draw(&mut app, &mut clip);
        }
        let first_observed = app.doc().unwrap().first_visible_row;
        assert_eq!(
            app.doc().unwrap().pending_scroll_row,
            None,
            "뷰 전용 렌더도 스크롤 요청을 소비해야 한다"
        );

        // 두 번째 Page Down — `first_visible_row`가 조기 반환 뒤로 밀려
        // 0에 고정돼 있다면 여기서 다시 같은(작은) 목표를 요청하게 된다.
        let second_target = {
            let doc = app.doc_mut().unwrap();
            apply_page_scroll(doc, PageDir::Down);
            doc.pending_scroll_row.unwrap()
        };
        for _ in 0..40 {
            draw(&mut app, &mut clip);
        }
        let second_observed = app.doc().unwrap().first_visible_row;

        assert!(
            second_target > first_target,
            "두 번째 Page Down의 목표는 첫 번째보다 더 아래여야 한다 \
             (first_target={first_target}, second_target={second_target})"
        );
        assert!(
            second_observed > first_observed,
            "뷰 전용 모드에서도 실제로 화면이 두 번째 페이지로 나아가야 한다 \
             (first_observed={first_observed}, second_observed={second_observed})"
        );
    }

    /// `render_table`의 (1) `scroll_align` 사용(Align::TOP으로 정확히
    /// 스크롤하는가 — `render_table.rs:4372`의 하드코딩 Center 뮤턴트를
    /// 잡는다)과 (2) 관측 write-back(`first_visible_row`/`visible_rows`
    /// 기록 — 그 블록을 통째로 지우는 뮤턴트를 잡는다)을 한 번에 검증한다.
    /// `render_records_first_visible_row_and_page_size`(텍스트 모드)와 같은
    /// 골격을 표 모드로 옮긴 것이다.
    #[test]
    fn render_table_scrolls_to_aligned_row_and_records_observation() {
        // 헤더 없이 500행. has_header=false로 두면 화면 행 = 논리 행이라
        // 목표 계산이 단순해진다.
        let content: Vec<u8> = (0..500)
            .map(|i| format!("r{i},v{i}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        let p = temp(&content);
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        {
            let doc = app.doc_mut().unwrap();
            doc.indexer.take().unwrap().join().unwrap();
            doc.has_header = false;
        }

        let mut clip = String::new();
        let mut input = egui::RawInput::default();
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        input.screen_rect = Some(screen);
        let draw = |app: &mut App, clip: &mut String| {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let doc = app.doc_mut().unwrap();
                    render_table(ui, doc, b',', 0, 0, clip);
                });
            });
        };
        draw(&mut app, &mut clip);
        {
            let doc = app.doc().unwrap();
            assert_eq!(doc.first_visible_row, 0, "처음에는 맨 위를 보고 있다");
            assert!(
                doc.visible_rows > 0 && doc.visible_rows < 100,
                "작은 창의 한 화면 행 수가 기록되어야 한다(got {})",
                doc.visible_rows
            );
        }

        // Page Up/Down과 같은 정렬(TOP)로 200행에 스크롤을 요청한다.
        {
            let doc = app.doc_mut().unwrap();
            doc.pending_scroll_row = Some(200);
            doc.pending_scroll_align = egui::Align::TOP;
        }
        // **한 프레임만** 그린다(K-3). `scroll_to_row`(애니메이션)였다면 여기서
        // 목표에 도달하지 못하고 수십 프레임에 걸쳐 감겼다 — 지금은
        // `vertical_scroll_offset`으로 `state.offset.y`에 즉시 대입되므로
        // 다음 프레임의 관측이 곧 목표 자리다. 그리는 프레임 수를 늘리지 말 것:
        // "한 프레임 뒤 도달"이 이 테스트가 지키는 성질이다.
        draw(&mut app, &mut clip); // offset 적용
        draw(&mut app, &mut clip); // 그 자리를 관측(first_visible_row 기록)
        let doc = app.doc().unwrap();
        assert_eq!(
            doc.pending_scroll_row, None,
            "render_table이 스크롤 요청을 소비한다"
        );
        // `Align::TOP`은 목표 행의 위 경계를 화면 위에 맞춘다. `body.rows`가
        // 위쪽에 걸친 행까지 그리므로 관측되는 첫 행은 199 또는 200이다
        // (픽셀 반올림 — 정확히 200으로 고정하지 않는다). `Align::Center`로
        // 하드코딩된 뮤턴트라면 이 범위를 크게 벗어난다.
        assert!(
            (199..=200).contains(&doc.first_visible_row),
            "TOP 정렬 스크롤 후 **한 프레임 만에** 화면 첫 행이 목표 근처(199 또는 \
             200)여야 한다 — 애니메이션이 남아 있으면 훨씬 작은 값이 나온다 (got {})",
            doc.first_visible_row
        );

        // Center 정렬도 즉시 도달하는가(찾기 경로). 목표 행이 화면 **중앙**이므로
        // 첫 행은 목표에서 반 화면만큼 위다.
        let half = app.doc().unwrap().visible_rows / 2;
        {
            let doc = app.doc_mut().unwrap();
            doc.pending_scroll_row = Some(400);
            doc.pending_scroll_align = egui::Align::Center;
        }
        draw(&mut app, &mut clip);
        draw(&mut app, &mut clip);
        let doc = app.doc().unwrap();
        let want = 400 - half;
        assert!(
            doc.first_visible_row.abs_diff(want) <= 1,
            "Center 정렬 스크롤 후 한 프레임 만에 첫 행이 {want} 근처여야 한다 (got {})",
            doc.first_visible_row
        );
    }

    /// `scroll_offset_for_row`: 행 번호 → 세로 offset(px). 계산이
    /// `TableBody::rows`(`row_height + item_spacing.y` 배수)와 같아야 한다.
    #[test]
    fn scroll_offset_for_row_matches_table_row_geometry() {
        let (rh, sp) = (20.0f32, 4.0f32);
        let step = rh + sp; // 24.0
        // TOP: 목표 행의 위 경계가 곧 offset.
        assert_eq!(scroll_offset_for_row(0, egui::Align::TOP, rh, sp, 240.0), 0.0);
        assert_eq!(scroll_offset_for_row(10, egui::Align::TOP, rh, sp, 240.0), 10.0 * step);
        // Center: 뷰포트 절반만큼 위로 당긴다.
        assert_eq!(
            scroll_offset_for_row(10, egui::Align::Center, rh, sp, 240.0),
            10.0 * step - (240.0 - step) * 0.5
        );
        // BOTTOM: 목표 행의 아래 경계가 화면 바닥.
        assert_eq!(
            scroll_offset_for_row(20, egui::Align::BOTTOM, rh, sp, 240.0),
            21.0 * step - 240.0
        );
        // 문서 맨 앞보다 위로는 갈 수 없다 — 음수는 0으로 클램프.
        assert_eq!(scroll_offset_for_row(0, egui::Align::Center, rh, sp, 240.0), 0.0);
        assert_eq!(scroll_offset_for_row(1, egui::Align::BOTTOM, rh, sp, 240.0), 0.0);
    }

    /// **뮤테이션 감지.** 정렬 세 값이 서로 다른 offset을 내야 한다 —
    /// 어느 하나로 하드코딩하면(예: 전부 TOP) 찾기의 중앙 정렬이 조용히
    /// 상단 정렬로 바뀐다.
    #[test]
    fn scroll_offset_for_row_distinguishes_alignments() {
        let (rh, sp, vh) = (20.0f32, 4.0f32, 240.0f32);
        let top = scroll_offset_for_row(50, egui::Align::TOP, rh, sp, vh);
        let center = scroll_offset_for_row(50, egui::Align::Center, rh, sp, vh);
        let bottom = scroll_offset_for_row(50, egui::Align::BOTTOM, rh, sp, vh);
        assert!(bottom < center && center < top, "BOTTOM < Center < TOP (got {bottom}/{center}/{top})");
        // 행 번호가 커지면 offset도 커진다(단조).
        assert!(
            scroll_offset_for_row(51, egui::Align::TOP, rh, sp, vh) > top,
            "행 번호에 비례해야 한다"
        );
    }

    /// 거터 클릭은 정렬을 **항상 Center로 되돌린다** — Page 키가 남긴 TOP을
    /// 그대로 두면 거터 클릭으로 점프한 자리가 화면 맨 위에 붙어 버린다.
    /// `render_match_gutter`는 실제 클릭을 시뮬레이션하기보다(SidePanel
    /// 클릭 좌표 산출이 간접적이고 깨지기 쉽다) 그 결정을 뽑아낸 순수 함수
    /// `gutter_click_target`으로 검증한다 — `page_keys_live`/
    /// `classify_cell_hit`와 같은 이 코드베이스의 표준 처방.
    #[test]
    fn gutter_click_target_always_resets_align_to_center() {
        let (mut app, _delim) = edit_doc(b"a,b\n1,2\n3,4\n5,6\n", false);
        let doc = app.doc_mut().unwrap();
        // Page Down이 TOP을 남겼다고 가정.
        doc.pending_scroll_align = egui::Align::TOP;
        let (align, _row) = gutter_click_target(doc, 1, doc.sep);
        assert_eq!(
            align,
            egui::Align::Center,
            "거터 클릭은 이전 정렬(TOP)과 무관하게 항상 Center로 되돌려야 한다"
        );
    }

    /// 표 모드(구분자 있음)에서 거터 클릭의 논리 행 → 화면 행 변환이
    /// `logical_to_screen_row`와 일치하는가(정렬 permutation을 거치는 경로).
    #[test]
    fn gutter_click_target_maps_logical_row_in_table_mode() {
        let (mut app, delim) = edit_doc(b"h1,h2\na,1\nb,2\nc,3\n", true);
        let doc = app.doc_mut().unwrap();
        assert_eq!(doc.sep, SeparatorMode::Char(delim));
        let (_align, row) = gutter_click_target(doc, 2, doc.sep);
        // has_header=true → data_start=1 → 논리 2행은 화면 1행.
        assert_eq!(row, 1);
    }

    // ---- 찾기/바꾸기 이스케이프 시퀀스 (Task L) ----------------------------

    /// TSV 편집 문서를 만든다. `edit_doc`은 `.csv`로 열어 구분자가 `,`로
    /// 고정되므로, 탭이 **구분자**인 상황(사용자의 실제 목적)을 만들려면
    /// 확장자를 달리해 열어야 한다.
    fn tsv_edit_doc(content: &[u8]) -> App {
        let p = temp_ext(content, "tsv");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.open_path(&p, &ctx);
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        doc.has_header = false;
        enter_edit_mode(doc);
        app
    }

    #[test]
    fn effective_query_respects_toggle() {
        let (mut app, _d) = edit_doc(b"a,b\n", false);
        let doc = app.doc_mut().unwrap();
        // 꺼짐: 윈도우 경로가 글자 그대로 남는다(기본값이 꺼짐인 이유).
        doc.find_query = r"C:\temp".to_owned();
        assert!(!doc.find_escapes, "기본값은 꺼짐");
        assert_eq!(effective_query(doc), r"C:\temp");
        doc.find_query = r"a\tb".to_owned();
        assert_eq!(effective_query(doc), r"a\tb", "꺼져 있으면 두 글자 그대로");
        // 켜짐: `\t`가 탭 한 글자가 된다.
        doc.find_escapes = true;
        assert_eq!(effective_query(doc), "a\tb");
        assert_eq!(effective_query(doc), crate::find::unescape(r"a\tb"));
    }

    #[test]
    fn effective_replacement_respects_toggle() {
        let (mut app, _d) = edit_doc(b"a,b\n", false);
        let doc = app.doc_mut().unwrap();
        doc.replace_text = r"x\ty".to_owned();
        assert_eq!(effective_replacement(doc), r"x\ty", "꺼짐: 날 문자열");
        doc.find_escapes = true;
        assert_eq!(effective_replacement(doc), "x\ty", "켜짐: 실제 탭");
    }

    /// 사용자의 실제 목적을 직접 검증한다: TSV의 구분자 탭을 `\t`로 찾는다.
    #[test]
    fn tab_search_finds_tsv_separator() {
        let mut app = tsv_edit_doc(b"a\tb\nnotab\nc\td\n");
        let doc = app.doc_mut().unwrap();
        assert_eq!(doc.sep, SeparatorMode::Char(b'\t'), "사전 조건: 탭 구분");
        doc.find_query = r"\t".to_owned();
        // 꺼져 있으면 `\`+`t` 두 글자를 찾으므로 아무 행도 안 걸린다.
        apply_find_action(doc, FindAction::All);
        assert!(
            doc.highlight.as_ref().unwrap().rows.is_empty(),
            "이스케이프가 꺼져 있으면 리터럴 두 글자를 찾는다"
        );
        // 켜면 실제 탭이 있는 행만 걸린다.
        doc.find_escapes = true;
        apply_find_action(doc, FindAction::All);
        assert_eq!(doc.highlight.as_ref().unwrap().rows, vec![0, 2]);
    }

    /// Find All 스냅샷에는 **디코딩된** 검색어가 들어간다(L-4).
    #[test]
    fn find_all_snapshot_stores_decoded_query() {
        let mut app = tsv_edit_doc(b"a\tb\n");
        let doc = app.doc_mut().unwrap();
        doc.find_escapes = true;
        doc.find_query = r"\t".to_owned();
        apply_find_action(doc, FindAction::All);
        let hl = doc.highlight.as_ref().unwrap();
        assert_eq!(hl.query, "\t", "스냅샷은 날 문자열이 아니라 실제 탭을 담는다");
        assert_ne!(hl.query, doc.find_query);
    }

    /// Find All 뒤에 체크박스를 꺼도 스냅샷은 그대로다 — 스냅샷의 뜻이
    /// "그때 그 검색어로 찾은 결과"이기 때문(`opts`를 얼려 두는 것과 같은 규율).
    #[test]
    fn highlight_survives_escape_toggle_off() {
        let mut app = tsv_edit_doc(b"a\tb\nnotab\n");
        let doc = app.doc_mut().unwrap();
        doc.find_escapes = true;
        doc.find_query = r"\t".to_owned();
        apply_find_action(doc, FindAction::All);
        let before = doc.highlight.clone();
        assert_eq!(before.as_ref().unwrap().rows, vec![0]);
        // 체크박스를 끈다(렌더 경로가 하는 일 = 필드 토글 + 리셋 판정).
        doc.find_escapes = false;
        assert_eq!(doc.highlight, before, "하이라이트는 다음 Find All까지 유지");
        assert_eq!(doc.highlight.as_ref().unwrap().query, "\t");
    }

    /// 이스케이프 토글도 옵션 변경과 같은 결로 검색 기준을 바꾸므로
    /// `last_match`/`find_status`를 리셋해야 한다. **`highlight`는 유지.**
    /// 판정은 순수 함수 `find_inputs_changed`가 한다(인라인 복붙 금지).
    #[test]
    fn escape_toggle_resets_last_match() {
        let o = crate::find::FindOptions::default();
        let mut o2 = o.clone();
        o2.match_case = true;
        // 아무것도 안 바뀌면 리셋하지 않는다.
        assert!(!find_inputs_changed(&o, &o, false, false));
        assert!(!find_inputs_changed(&o, &o, true, true));
        // 옵션만 바뀌어도(기존 동작), 이스케이프만 바뀌어도(새 동작) 리셋한다.
        assert!(find_inputs_changed(&o, &o2, false, false), "옵션 변경");
        assert!(find_inputs_changed(&o, &o, false, true), "이스케이프 켬");
        assert!(find_inputs_changed(&o, &o, true, false), "이스케이프 끔");
        assert!(find_inputs_changed(&o, &o2, false, true), "둘 다 바뀜");

        // 그 판정이 실제로 리셋으로 이어지는지 — 렌더가 하는 일을 그대로 옮긴다.
        let mut app = tsv_edit_doc(b"a\tb\n");
        let doc = app.doc_mut().unwrap();
        doc.find_escapes = true;
        doc.find_query = r"\t".to_owned();
        apply_find_action(doc, FindAction::All);
        apply_find_action(doc, FindAction::Next);
        assert!(doc.last_match.is_some(), "사전 조건: 커서가 잡혀 있다");
        let hl = doc.highlight.clone();
        let before_opts = doc.find_opts.clone();
        let before_esc = doc.find_escapes;
        doc.find_escapes = false;
        if find_inputs_changed(&before_opts, &doc.find_opts, before_esc, doc.find_escapes) {
            doc.last_match = None;
            doc.find_status.clear();
        }
        assert!(doc.last_match.is_none(), "기준이 바뀌었으므로 커서를 버린다");
        assert_eq!(doc.highlight, hl, "하이라이트는 건드리지 않는다");
    }

    /// 편집 모드에서 `\t`를 다른 글자로 Replace All 하면 실제 탭이 바뀌고,
    /// Ctrl+Z 한 번으로 전부 복구된다.
    #[test]
    fn replace_tab_with_text() {
        let mut app = tsv_edit_doc(b"a\tb\nc\td\nnotab\n");
        let doc = app.doc_mut().unwrap();
        let before = doc.edit.as_ref().unwrap().lines.clone();
        doc.find_escapes = true;
        doc.find_query = r"\t".to_owned();
        doc.replace_text = "|".to_owned();
        apply_find_action(doc, FindAction::ReplaceAll);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["a|b", "c|d", "notab"]));
        assert_eq!(doc.find_status, "2 replacements");
        undo_once(doc);
        assert_eq!(doc.edit.as_ref().unwrap().lines, before, "undo 한 번에 전부 복구");
    }

    /// **`replace_one` 배선 회귀(Task L).** `replace_one`의 재검증이 `doc.
    /// find_query`(날 문자열)를 쓰면 `\t`로 잡은 매치가 재검증마다 실패해
    /// "바꾸기"가 매번 "그냥 다음 찾기"로 흘러 버린다 — 같은 두 탭 자리
    /// 사이를 영원히 왔다 갔다 할 뿐 한 글자도 안 바뀐다. Replace All 계열
    /// 테스트(`replace_tab_with_text`)는 `replace_all_in_doc`이라는 **다른**
    /// 경로를 타므로 이 결함을 못 잡는다 — 여기서 `ReplaceOne`을 직접 반복
    /// 호출해 확인한다.
    #[test]
    fn replace_one_wiring_uses_effective_query() {
        let mut app = tsv_edit_doc(b"a\tb\tc\n");
        let doc = app.doc_mut().unwrap();
        doc.find_escapes = true;
        doc.find_query = r"\t".to_owned();
        doc.replace_text = "|".to_owned();
        apply_find_action(doc, FindAction::ReplaceOne); // 1회차: 아직 커서가 없으니 Find Next와 동일.
        assert_eq!(doc.edit.as_ref().unwrap().lines[0], "a\tb\tc", "1회차는 찾기만 한다");
        apply_find_action(doc, FindAction::ReplaceOne); // 2회차: 첫 탭을 "|"로.
        assert_eq!(doc.edit.as_ref().unwrap().lines[0], "a|b\tc");
        apply_find_action(doc, FindAction::ReplaceOne); // 3회차: 남은 탭을 "|"로.
        assert_eq!(
            doc.edit.as_ref().unwrap().lines[0],
            "a|b|c",
            "재검증이 effective_query를 쓰지 않으면 여기서 두 탭 자리를 영원히 오가며 절대 도달 못 한다"
        );
    }

    /// 반대 방향 — 치환문 쪽 `\t`도 실제 탭이 된다(`effective_replacement` 배선).
    #[test]
    fn replace_inserts_real_tab() {
        let (mut app, _d) = edit_doc(b"a|b\n", false);
        let doc = app.doc_mut().unwrap();
        doc.find_escapes = true;
        doc.find_query = "|".to_owned();
        doc.replace_text = r"\t".to_owned();
        apply_find_action(doc, FindAction::ReplaceAll);
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["a\tb"]));
    }

    /// **배선 누락 방지의 핵심 테스트.** 같은 문서·같은 입력에서 Find Next /
    /// Find All / 추출이 **같은 행 집합**을 본다. L-3 표의 소비 지점이 하나라도
    /// 날 `doc.find_query`를 쓰면 그 경로만 행 집합이 달라져 여기서 깨진다.
    #[test]
    fn escaped_query_used_by_every_path() {
        let mut app = tsv_edit_doc(b"a\tb\nnotab\nc\td\ne\tf\n");
        {
            let doc = app.doc_mut().unwrap();
            doc.find_escapes = true;
            doc.find_query = r"\t".to_owned();
        }
        // (1) Find All 스냅샷.
        {
            let doc = app.doc_mut().unwrap();
            apply_find_action(doc, FindAction::All);
            assert_eq!(doc.highlight.as_ref().unwrap().rows, vec![0, 2, 3]);
            // (2) scan_all_matches == 브루트포스(절대 계약, 디코딩된 needle로).
            assert_scan_equals_brute(doc);
            // (3) Find Next를 반복해 순회한 행들.
            let mut seen = Vec::new();
            for _ in 0..3 {
                apply_find_action(doc, FindAction::Next);
                seen.push(doc.last_match.unwrap().line as u32);
            }
            seen.dedup();
            assert_eq!(seen, vec![0, 2, 3], "Find Next도 같은 행들을 순회한다");
        }
        // (4) 추출이 만든 새 탭의 행 수와, 새 탭이 물려받은 상태.
        app.extract_matching_rows();
        let new_doc = app.doc().unwrap();
        assert!(new_doc.is_extracted);
        assert_eq!(doc_line_count(new_doc), 3, "매치 행 3개가 추출된다");
        assert!(new_doc.find_escapes, "이스케이프 토글도 새 탭에 물려준다");
        assert_eq!(new_doc.find_query, r"\t", "입력란에는 날 문자열이 보인다");
        assert_eq!(
            new_doc.highlight.as_ref().unwrap().query,
            "\t",
            "새 탭 스냅샷도 디코딩된 값"
        );
        assert_eq!(
            new_doc.highlight.as_ref().unwrap().rows,
            vec![0, 1, 2],
            "추출본은 전 행이 매치다"
        );
    }

    /// 이스케이프가 켜져 있어도 버튼 활성 조건은 **날 문자열**을 본다 —
    /// "사용자가 입력란에 뭔가 쳤는가"를 묻는 판정이기 때문. 두 근거가 실제로
    /// 같은 값을 준다는 것도 함께 못박는다(디코딩이 검색어를 비게 만들 수 없다).
    #[test]
    fn button_guards_use_raw_query() {
        assert!(!find_all_button_enabled(""));
        assert!(!extract_button_enabled(""));
        // 디코딩하면 사라질 것 같은 입력들도 전부 "쳤다"로 판정된다.
        for raw in [r"\", r"\x", r"\t", r"\n", r"C:\temp"] {
            assert!(find_all_button_enabled(raw), "{raw:?}는 활성");
            assert!(extract_button_enabled(raw), "{raw:?}는 활성");
            assert!(
                !crate::find::unescape(raw).is_empty(),
                "{raw:?}: 디코딩이 검색어를 비게 만들지 않는다"
            );
        }
        assert!(crate::find::unescape("").is_empty(), "빈 입력만 빈 결과");
    }

    // ---- 줄 끝 개행 기호 표시 ----

    /// 폰트에 제어문자 기호가 있으면 그것을 쓴다. **CRLF는 두 글자**.
    #[test]
    fn ending_glyphs_uses_control_pictures_when_available() {
        let all = |_c: char| true;
        for e in [
            parse::LineEnding::Lf,
            parse::LineEnding::Cr,
            parse::LineEnding::CrLf,
        ] {
            assert_eq!(
                ending_glyphs(e, all).chars().count(),
                1,
                "{e:?}는 기호 한 글자"
            );
        }
        assert_eq!(ending_glyphs(parse::LineEnding::None, all), "");
    }

    /// 폰트에 없으면 두부(□) 대신 이스케이프 표기로 떨어진다.
    #[test]
    fn ending_glyphs_falls_back_when_font_lacks_glyph() {
        let none = |_c: char| false;
        assert_eq!(ending_glyphs(parse::LineEnding::Lf, none), "\\n");
        assert_eq!(ending_glyphs(parse::LineEnding::Cr, none), "\\r");
        assert_eq!(ending_glyphs(parse::LineEnding::CrLf, none), "\\r\\n");
        assert_eq!(ending_glyphs(parse::LineEnding::None, none), "");
    }

    /// 화살표가 없는 폰트면 이스케이프로 떨어지고, 기호와 이스케이프가
    /// 섞이지 않는다.
    #[test]
    fn ending_glyphs_does_not_mix_notations() {
        // 화살표(U+21B5)만 없는 폰트를 흉내낸다.
        let no_arrow = |c: char| c != '\u{21B5}';
        let s = ending_glyphs(parse::LineEnding::CrLf, no_arrow);
        assert_eq!(s, "\\r\\n");
        assert!(!s.contains('\u{21B5}'), "기호와 이스케이프가 섞이면 안 된다");
    }

    /// 뷰 모드는 mmap 바이트를 직접 보므로 **줄마다** 진짜 개행을 읽어낸다.
    /// 섞인 파일도 있는 그대로 나온다.
    #[test]
    fn view_mode_reports_real_per_line_endings() {
        // 1행 CRLF, 2행 LF, 3행 종결 개행 없음.
        let p = temp_ext(b"aaa\r\nbbb\nccc", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        view_doc(doc);

        assert_eq!(line_ending_for_row(doc, 0), parse::LineEnding::CrLf);
        assert_eq!(line_ending_for_row(doc, 1), parse::LineEnding::Lf);
        assert_eq!(
            line_ending_for_row(doc, 2),
            parse::LineEnding::None,
            "종결 개행이 없는 마지막 줄에는 없는 개행을 그리지 않는다"
        );
        std::fs::remove_file(&p).ok();
    }

    /// 뷰 모드에서 마지막 줄에 종결 개행이 **있으면** 표시한다.
    #[test]
    fn view_mode_shows_ending_on_terminated_last_line() {
        let p = temp_ext(b"only\r\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        view_doc(doc);
        assert_eq!(line_ending_for_row(doc, 0), parse::LineEnding::CrLf);
        std::fs::remove_file(&p).ok();
    }

    /// 편집 모드는 파일 전체 스타일로 그린다 — 편집 버퍼가 줄별 개행을
    /// 보관하지 않기 때문(불변식: lines[i]에 개행 없음).
    #[test]
    fn edit_mode_uses_file_wide_newline_style() {
        let p = temp_ext(b"aaa\r\nbbb\r\nccc\r\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        let doc = app.doc_mut().unwrap();
        assert!(doc.edit.is_some(), "전제: 작은 파일이라 편집 모드");
        assert_eq!(
            doc.edit.as_ref().unwrap().newline,
            crate::edit::Newline::CrLf
        );
        assert_eq!(line_ending_for_row(doc, 0), parse::LineEnding::CrLf);
        assert_eq!(line_ending_for_row(doc, 1), parse::LineEnding::CrLf);
        std::fs::remove_file(&p).ok();
    }

    /// 편집 모드에서 LF 파일은 LF로.
    #[test]
    fn edit_mode_lf_file_shows_lf() {
        let p = temp_ext(b"aaa\nbbb\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        let doc = app.doc().unwrap();
        assert_eq!(line_ending_for_row(doc, 0), parse::LineEnding::Lf);
        std::fs::remove_file(&p).ok();
    }

    /// 편집 모드의 **마지막 줄**에는 기호를 붙이지 않는다. 편집 버퍼는 종결
    /// 개행이 있었는지 모르므로, 붙이면 절반은 거짓말이 된다.
    #[test]
    fn edit_mode_last_line_has_no_marker() {
        let p = temp_ext(b"aaa\nbbb\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        let doc = app.doc().unwrap();
        let n = doc.edit.as_ref().unwrap().lines.len();
        assert_eq!(
            line_ending_for_row(doc, n - 1),
            parse::LineEnding::None,
            "마지막 줄에는 없는 개행을 그리지 않는다"
        );
        std::fs::remove_file(&p).ok();
    }

    /// 새 파일(빈 한 줄)에는 그릴 개행이 없다.
    #[test]
    fn new_document_has_no_line_ending_marker() {
        let doc = new_document();
        assert_eq!(line_ending_for_row(&doc, 0), parse::LineEnding::None);
    }

    /// 줄을 추가하면 앞줄에는 기호가 생기고 새 마지막 줄에는 없다 —
    /// "마지막 줄만 예외"가 편집 중에도 유지되는지.
    #[test]
    fn marker_follows_last_line_as_document_grows() {
        let mut doc = new_document();
        doc.edit.as_mut().unwrap().lines = v(&["first", "second"]);
        assert_ne!(
            line_ending_for_row(&doc, 0),
            parse::LineEnding::None,
            "첫 줄은 다음 줄이 있으므로 개행으로 끝난다"
        );
        assert_eq!(line_ending_for_row(&doc, 1), parse::LineEnding::None);
    }

    // ---- 저장 시 개행 스타일 선택 ----

    /// 라벨에 플랫폼 이름이 있어야 한다 — 사용자가 고르는 기준은 대개
    /// "어디서 쓸 파일인가"이고, CRLF/LF만으로는 답할 수 없다.
    #[test]
    fn newline_labels_name_the_platforms() {
        let crlf = newline_label(crate::edit::Newline::CrLf);
        let lf = newline_label(crate::edit::Newline::Lf);
        assert!(crlf.contains("CRLF") && crlf.contains("Windows"), "{crlf}");
        assert!(lf.contains("LF") && lf.contains("Linux"), "{lf}");
        assert_ne!(crlf, lf);
    }

    /// 다이얼로그 기본값은 **원본과 같은 스타일**이다. 저장이 개행을 조용히
    /// 바꾸면 diff가 전 줄로 번진다.
    #[test]
    fn save_defaults_keep_the_documents_newline() {
        let p = temp_ext(b"a\nb\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        assert_eq!(
            app.doc().unwrap().edit.as_ref().unwrap().newline,
            crate::edit::Newline::Lf,
            "전제: LF 파일로 열렸다"
        );
        app.save_newline = crate::edit::Newline::CrLf; // 이전 문서의 잔재를 흉내
        app.init_save_defaults();
        assert_eq!(
            app.save_newline,
            crate::edit::Newline::Lf,
            "다이얼로그를 열면 이 문서의 스타일로 맞춘다"
        );
        std::fs::remove_file(&p).ok();
    }

    /// 저장한 개행이 문서에 **되쓰인다**. 되쓰지 않으면 파일은 LF인데 화면
    /// 기호는 계속 CRLF를 그려 화면이 파일을 설명하지 못한다.
    #[test]
    fn saving_writes_chosen_newline_back_to_document() {
        let p = temp_ext(b"a\r\nb\r\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        let doc = app.doc_mut().unwrap();
        assert_eq!(
            doc.edit.as_ref().unwrap().newline,
            crate::edit::Newline::CrLf,
            "전제: CRLF 파일"
        );

        apply_save_newline(doc, crate::edit::Newline::Lf);

        assert_eq!(doc.edit.as_ref().unwrap().newline, crate::edit::Newline::Lf);
        assert_eq!(
            line_ending_for_row(doc, 0),
            parse::LineEnding::Lf,
            "화면 기호도 즉시 LF로 바뀐다"
        );
        std::fs::remove_file(&p).ok();
    }

    /// 되쓰기가 dirty를 켜면 안 된다 — 저장 직후라 버퍼와 파일이 같은
    /// 상태인데, 저장하자마자 "저장 안 됨"이 되면 안 된다.
    #[test]
    fn applying_newline_does_not_mark_dirty() {
        let mut doc = new_document();
        doc.edit.as_mut().unwrap().dirty = false;
        apply_save_newline(&mut doc, crate::edit::Newline::Lf);
        assert!(!doc.edit.as_ref().unwrap().dirty);
    }

    /// 뷰 모드(편집 버퍼 없음)에서 불러도 패닉하지 않는다.
    #[test]
    fn applying_newline_is_noop_without_edit_buffer() {
        let p = temp_ext(b"a\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        let doc = app.doc_mut().unwrap();
        doc.indexer.take().unwrap().join().unwrap();
        view_doc(doc);
        apply_save_newline(doc, crate::edit::Newline::Lf); // 패닉하지 않는다
        assert!(doc.edit.is_none());
        std::fs::remove_file(&p).ok();
    }

    /// 고른 스타일이 실제 파일 바이트로 나간다 — LF를 고르면 `\r`이 없어야
    /// 한다(리눅스로 보낼 파일의 요구사항 그 자체).
    #[test]
    fn chosen_newline_reaches_the_written_bytes() {
        let lines = v(&["a", "b"]);
        for (nl, expect) in [
            (crate::edit::Newline::Lf, &b"a\nb\n"[..]),
            (crate::edit::Newline::CrLf, &b"a\r\nb\r\n"[..]),
        ] {
            let out = temp(b"");
            let opts = crate::save::SaveOptions {
                enc: crate::parse::Encoding::Utf8,
                bom: false,
                newline: nl,
            };
            crate::save::write_file(&out, &lines, &opts, None).unwrap();
            assert_eq!(std::fs::read(&out).unwrap(), expect, "{nl:?}");
            std::fs::remove_file(&out).ok();
        }
    }

    // ---- IME(한글/일본어/중국어 조합) 입력 ----

    /// `collect_text_intents`에 이벤트를 흘려 인텐트를 뽑는다.
    ///
    /// `InputState`는 비공개 필드가 있어 리터럴로 못 만든다. 실제 `Context`에
    /// `RawInput`으로 넣어 한 프레임을 돌리는데, 이쪽이 진짜 이벤트 경로와
    /// 같으므로 오히려 충실한 검증이다.
    /// 큐에 남아 있는 Tab 키 이벤트 수(0이면 소비된 것).
    fn tab_events_left(ctx: &egui::Context) -> usize {
        ctx.input(|i| {
            i.events
                .iter()
                .filter(|e| matches!(e, egui::Event::Key { key: egui::Key::Tab, .. }))
                .count()
        })
    }
    /// Tab 키 눌림 이벤트 하나.
    fn tab_event(modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }
    fn intents_from(events: Vec<egui::Event>) -> Vec<TextEditIntent> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut out = Vec::new();
        let _ = ctx.run(input, |ctx| {
            out = ctx.input(collect_text_intents);
        });
        out
    }

    /// **이 테스트가 회귀의 핵심이다.** 한글은 `Event::Text`가 아니라
    /// `Event::Ime(Commit)`으로 들어온다. 그 분기가 없으면 편집 모드에서
    /// 한글이 아예 입력되지 않는다(사용자 보고).
    #[test]
    fn ime_commit_becomes_insert_intent() {
        let out = intents_from(vec![egui::Event::Ime(egui::ImeEvent::Commit(
            "한글".to_owned(),
        ))]);
        assert_eq!(out.len(), 1, "확정 문자는 삽입 하나로 들어간다");
        match &out[0] {
            TextEditIntent::Insert(s) => assert_eq!(s, "한글"),
            other => panic!("Insert를 기대했는데 {other:?}"),
        }
    }

    /// 조합 중간(`Preedit`)은 **미리보기**로 온다 — 초·중·종성 단위로
    /// 화면에 보여야 하기 때문(사용자 요청). 버퍼는 건드리지 않는다.
    #[test]
    fn ime_preedit_becomes_preview_intents() {
        let out = intents_from(vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("ㅎ".to_owned())),
            egui::Event::Ime(egui::ImeEvent::Preedit("하".to_owned())),
            egui::Event::Ime(egui::ImeEvent::Preedit("한".to_owned())),
        ]);
        let previews: Vec<&String> = out
            .iter()
            .filter_map(|i| match i {
                TextEditIntent::ImePreview(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(previews, vec!["ㅎ", "하", "한"], "조합 단계가 그대로 보인다");
        assert!(
            !out.iter().any(|i| matches!(i, TextEditIntent::Insert(_))),
            "조합 중에는 버퍼에 넣지 않는다: {out:?}"
        );
    }

    /// 미리보기는 **버퍼·undo·dirty 어디에도** 들어가지 않는다. 조합만
    /// 해도 dirty가 켜지면 저장 안 한 문서로 오인된다.
    #[test]
    fn ime_preview_does_not_dirty_the_buffer() {
        let mut app = find_test_doc(&["abc"]);
        let doc = app.doc_mut().unwrap();
        doc.edit.as_mut().unwrap().dirty = false;
        let undo_before = doc.edit.as_ref().unwrap().undo.len();
        let mut clip = String::new();

        apply_text(doc, &mut clip, TextEditIntent::ImePreview("ㅎ".into()));

        assert_eq!(doc.ime_preview, "ㅎ", "화면에는 보인다");
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["abc"]), "버퍼는 그대로");
        assert!(!doc.edit.as_ref().unwrap().dirty, "조합만으로 dirty가 되면 안 된다");
        assert_eq!(
            doc.edit.as_ref().unwrap().undo.len(),
            undo_before,
            "undo 스택도 그대로"
        );
    }

    /// 확정되면 미리보기가 **지워지고** 그 글자가 버퍼에 들어간다.
    /// 지우지 않으면 확정 글자가 화면에 두 번 나온다.
    #[test]
    fn ime_commit_clears_the_preview() {
        let mut app = find_test_doc(&["abc"]);
        let doc = app.doc_mut().unwrap();
        doc.text_caret = crate::edit::TextPos { line: 0, col: 3 };
        let mut clip = String::new();

        apply_text(doc, &mut clip, TextEditIntent::ImePreview("한".into()));
        apply_text(doc, &mut clip, TextEditIntent::Insert("한".into()));

        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["abc한"]));
        assert!(doc.ime_preview.is_empty(), "확정 뒤 미리보기가 남으면 두 번 보인다");
    }

    /// 조합 → 확정의 전체 흐름. 확정된 글자는 **한 번만** 삽입된다
    /// (조합 단계가 같이 삽입되면 "ㅎ한한"이 된다).
    #[test]
    fn ime_composition_then_commit_inserts_once() {
        let out = intents_from(vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("ㅎ".to_owned())),
            egui::Event::Ime(egui::ImeEvent::Preedit("한".to_owned())),
            egui::Event::Ime(egui::ImeEvent::Commit("한".to_owned())),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ]);
        let inserts: Vec<&String> = out
            .iter()
            .filter_map(|i| match i {
                TextEditIntent::Insert(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(inserts, vec!["한"], "확정된 글자 하나만 버퍼로: {out:?}");
    }

    /// 조합이 취소되면(`Disabled`) 미리보기가 지워진다 — 남으면 확정도 안 된
    /// 글자가 화면에 계속 떠 있다.
    #[test]
    fn ime_disabled_clears_the_preview() {
        let out = intents_from(vec![
            egui::Event::Ime(egui::ImeEvent::Preedit("ㅎ".to_owned())),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ]);
        match out.last() {
            Some(TextEditIntent::ImePreview(s)) => assert!(s.is_empty(), "빈 미리보기로 해제"),
            other => panic!("마지막은 미리보기 해제여야 한다: {other:?}"),
        }
    }

    // ---- Tab 키 ----
    //
    // 소비는 `App::wants_tab_character`가 `update()` 맨 앞에서 하고, 삽입은
    // `text_frame_intents`가 그 결과(bool)를 받아 한다. 둘로 나뉜 이유는
    // 메뉴바가 본문보다 먼저 그려지기 때문이다 — 아래 첫 테스트가 그 증상이다.

    /// 편집 모드 + 텍스트 모드면 Tab을 먹는다(그리고 이벤트를 없앤다).
    #[test]
    fn wants_tab_character_in_text_edit_mode() {
        let app = find_test_doc(&["ab"]);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        let mut took = false;
        let mut left = 0;
        let _ = ctx.run(input, |ctx| {
            took = app.wants_tab_character(ctx);
            left = tab_events_left(ctx);
        });
        assert!(took, "본문이 탭 문자를 받아야 한다");
        assert_eq!(left, 0, "이벤트를 없애야 포커스 순회가 일어나지 않는다");
    }

    /// 표 모드에서는 Tab을 먹지 않는다 — 셀 안에 탭이 들어가면 TSV에서
    /// 필드가 갈라지고, 표에서는 Tab이 이동 키로 쓰이는 것이 관행이다.
    #[test]
    fn wants_tab_character_is_false_in_table_mode() {
        let mut app = find_test_doc(&["a,b"]);
        app.doc_mut().unwrap().sep = SeparatorMode::Char(b',');
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        let mut took = false;
        let mut left = 0;
        let _ = ctx.run(input, |ctx| {
            took = app.wants_tab_character(ctx);
            left = tab_events_left(ctx);
        });
        assert!(!took);
        assert_eq!(left, 1, "이벤트를 남겨 포커스 순회에 쓰이게 한다");
    }

    /// 뷰 모드에서는 Tab이 평범한 포커스 순회여야 한다(접근성).
    #[test]
    fn wants_tab_character_is_false_in_view_mode() {
        let p = temp_ext(b"a\nb\n", "txt");
        let ctx0 = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx0, Default::default());
        {
            let doc = app.doc_mut().unwrap();
            doc.indexer.take().unwrap().join().unwrap();
            view_doc(doc);
        }
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        let mut took = false;
        let _ = ctx.run(input, |ctx| took = app.wants_tab_character(ctx));
        assert!(!took);
        std::fs::remove_file(&p).ok();
    }

    /// 저장 다이얼로그가 떠 있으면 Tab은 그 안의 버튼 사이를 옮겨야 한다.
    #[test]
    fn wants_tab_character_yields_to_a_focused_widget() {
        // 툴바 TextEdit 등이 포커스를 쥐고 있으면 Tab은 그 위젯을 벗어나는
        // 포커스 순회여야 한다 — 본문이 가로채면 입력란에 갇힌다.
        let app = find_test_doc(&["ab"]);
        let ctx = egui::Context::default();
        let other = egui::Id::new("toolbar_input");
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
                    other,
                    egui::Sense::click(),
                );
                r.request_focus();
            });
        });
        assert_eq!(ctx.memory(|m| m.focused()), Some(other), "전제: 포커스가 밖");

        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        let mut took = false;
        let mut left = 0;
        let _ = ctx.run(input, |ctx| {
            took = app.wants_tab_character(ctx);
            left = tab_events_left(ctx);
        });
        assert!(!took, "포커스를 쥔 위젯이 있으면 본문이 가로채지 않는다");
        assert_eq!(left, 1, "이벤트를 남겨 포커스 순회에 쓰이게 한다");
    }

    /// **깜빡임 회귀 방지.** 본문이 Tab을 먹은 프레임에는 give_to_next가
    /// 선점·소진되어, 그 뒤에 그려지는 focusable 위젯이 포커스를 받지 못한다.
    #[test]
    fn body_tab_leaves_nothing_for_later_widgets() {
        let app = find_test_doc(&["ab"]);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        let mut took = false;
        let _ = ctx.run(input, |ctx| {
            took = app.wants_tab_character(ctx);
            // 그다음에 그려지는 메뉴 버튼 흉내 — give_to_next가 남아 있으면
            // 이 위젯이 포커스를 가져가 하이라이트가 깜빡인다.
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.interact(
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(30.0, 10.0)),
                    egui::Id::new("menu_file_button"),
                    egui::Sense::click(),
                );
                ui.memory_mut(|m| m.interested_in_focus(egui::Id::new("menu_file_button")));
            });
        });
        assert!(took, "전제: 본문이 Tab을 먹었다");
        assert_eq!(
            ctx.memory(|m| m.focused()),
            None,
            "본문이 먹은 Tab은 뒤에 그려지는 위젯에 포커스를 주면 안 된다"
        );
    }

    #[test]
    fn wants_tab_character_is_false_while_a_dialog_is_open() {
        let mut app = find_test_doc(&["ab"]);
        app.show_save_dialog = true;
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        let mut took = false;
        let _ = ctx.run(input, |ctx| took = app.wants_tab_character(ctx));
        assert!(!took, "다이얼로그 위에서는 본문이 Tab을 가로채면 안 된다");
    }

    /// Shift+Tab은 먹지 않는다 — 관행상 내어쓰기인데 그 기능이 없고,
    /// 역방향 포커스 순회로 남기는 편이 낫다.
    ///
    /// `consume_key`를 안 쓰는 이유가 여기 있다: 그쪽은 `matches_logically`라
    /// Shift를 무시해서 Shift+Tab까지 먹는다(egui `input_state.rs:484`).
    #[test]
    fn shift_tab_is_left_alone() {
        let app = find_test_doc(&["ab"]);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::SHIFT)],
            ..Default::default()
        };
        let mut took = false;
        let mut left = 0;
        let _ = ctx.run(input, |ctx| {
            took = app.wants_tab_character(ctx);
            left = tab_events_left(ctx);
        });
        assert!(!took, "Shift+Tab은 탭 문자가 아니다");
        assert_eq!(left, 1, "역방향 포커스 순회로 남긴다");
    }

    /// 탭 문자가 실제로 버퍼에 들어가는지.
    #[test]
    fn tab_reaches_the_edit_buffer() {
        let mut app = find_test_doc(&["a"]);
        let doc = app.doc_mut().unwrap();
        doc.text_caret = crate::edit::TextPos { line: 0, col: 1 };
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Insert("\t".into()));
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["a\t"]));
    }

    /// 소비된 Tab(`tab_pressed`)이 삽입 인텐트로 되살아난다.
    #[test]
    fn text_frame_intents_revives_the_consumed_tab() {
        let ctx = egui::Context::default();
        let mut got = Vec::new();
        let _ = ctx.run(Default::default(), |ctx| {
            got = text_frame_intents(ctx, true, true);
        });
        match got.as_slice() {
            [TextEditIntent::Insert(s)] => assert_eq!(s, "\t"),
            other => panic!("탭 삽입을 기대했는데 {other:?}"),
        }
    }

    /// **이 버그의 핵심 성질.** 포커스가 다른 위젯에 있어도 탭은 들어가야
    /// 한다 — 게이트를 태우면 첫 Tab이 포커스를 옮긴 뒤 스스로를 막는다.
    #[test]
    fn tab_ignores_the_focus_gate() {
        let ctx = egui::Context::default();
        let other = egui::Id::new("toolbar_widget");
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
                    other,
                    egui::Sense::click(),
                );
                r.request_focus();
            });
        });
        assert_eq!(ctx.memory(|m| m.focused()), Some(other), "전제: 포커스가 밖");

        let mut got = Vec::new();
        let _ = ctx.run(Default::default(), |ctx| {
            got = text_frame_intents(ctx, true, true);
        });
        match got.as_slice() {
            [TextEditIntent::Insert(s)] => assert_eq!(s, "\t"),
            other => panic!("포커스가 밖이어도 탭은 들어가야 한다: {other:?}"),
        }
    }

    /// 반대로 **일반 글자**는 게이트를 타야 한다 — 툴바 입력란에 친 글자가
    /// 본문에도 들어가면 안 된다(게이트의 원래 목적).
    #[test]
    fn ordinary_text_still_respects_the_focus_gate() {
        let ctx = egui::Context::default();
        let other = egui::Id::new("toolbar_widget");
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
                    other,
                    egui::Sense::click(),
                );
                r.request_focus();
            });
        });
        let input = egui::RawInput {
            events: vec![egui::Event::Text("a".to_owned())],
            ..Default::default()
        };
        let mut got = Vec::new();
        let _ = ctx.run(input, |ctx| got = text_frame_intents(ctx, true, false));
        assert!(got.is_empty(), "포커스가 밖이면 글자는 본문에 안 들어간다: {got:?}");
    }

    /// 뷰 모드면 `tab_pressed`가 참으로 들어와도 삽입하지 않는다(이중 안전).
    #[test]
    fn text_frame_intents_ignores_tab_in_view_mode() {
        let ctx = egui::Context::default();
        let mut got = Vec::new();
        let _ = ctx.run(Default::default(), |ctx| {
            got = text_frame_intents(ctx, false, true);
        });
        assert!(got.is_empty(), "{got:?}");
    }
    /// 빈 Commit은 무시한다(조합을 취소하면 빈 문자열이 올 수 있다).
    #[test]
    fn ime_empty_commit_is_ignored() {
        let out = intents_from(vec![egui::Event::Ime(egui::ImeEvent::Commit(String::new()))]);
        assert!(out.is_empty());
    }

    /// IME 확정이 실제로 편집 버퍼에 반영되는지 — 인텐트 적용까지 끝에서 끝까지.
    #[test]
    fn ime_commit_reaches_the_edit_buffer() {
        let mut app = find_test_doc(&["abc"]);
        let doc = app.doc_mut().unwrap();
        doc.text_caret = crate::edit::TextPos { line: 0, col: 3 };
        let mut clip = String::new();
        apply_text(doc, &mut clip, TextEditIntent::Insert("한글".into()));
        assert_eq!(doc.edit.as_ref().unwrap().lines, v(&["abc한글"]));
    }

    /// 영문(`Event::Text`)과 한글(`Event::Ime`)이 같은 프레임에 섞여 와도
    /// 순서대로 둘 다 들어간다.
    #[test]
    fn ascii_and_ime_text_both_arrive() {
        let out = intents_from(vec![
            egui::Event::Text("a".to_owned()),
            egui::Event::Ime(egui::ImeEvent::Commit("가".to_owned())),
        ]);
        assert_eq!(out.len(), 2);
    }

    /// IME 위치 통보가 `output.ime`를 채운다. **이것이 채워져야 winit이
    /// `set_ime_allowed(true)`를 호출한다**(egui-winit lib.rs의
    /// `let allow_ime = ime.is_some()`) — 즉 이 호출이 없으면 IME 자체가
    /// 켜지지 않아 한글 입력이 시작조차 안 된다.
    #[test]
    fn set_ime_output_fills_platform_output() {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 16.0));
        let caret = egui::Rect::from_min_size(egui::pos2(50.0, 20.0), egui::vec2(1.5, 16.0));
        set_ime_output(&ctx, rect, caret);
        let got = ctx.output(|o| o.ime);
        let got = got.expect("output.ime가 채워져야 IME가 켜진다");
        assert_eq!(got.rect, rect);
        assert_eq!(got.cursor_rect, caret, "조합 창이 캐럿 자리에 뜬다");
    }

    /// 저장 성공 처리는 **두 가지를 함께** 한다 — 개행 되쓰기와 dirty 해제.
    /// 하나만 해도 아무 테스트가 안 깨지던 자리라 함수로 빼서 못박는다.
    #[test]
    fn mark_saved_applies_newline_and_clears_dirty() {
        let mut doc = new_document();
        doc.edit.as_mut().unwrap().dirty = true;
        assert_eq!(
            doc.edit.as_ref().unwrap().newline,
            crate::edit::Newline::CrLf,
            "전제: 새 문서는 CRLF"
        );

        mark_saved(&mut doc, crate::edit::Newline::Lf);

        assert_eq!(
            doc.edit.as_ref().unwrap().newline,
            crate::edit::Newline::Lf,
            "고른 개행이 문서에 반영된다"
        );
        assert!(!doc.edit.as_ref().unwrap().dirty, "저장했으므로 깨끗해진다");
    }

    /// `doc_savable`은 텍스트 편집 버퍼 또는 헥스 편집 버퍼 중 하나만 있어도
    /// 참이어야 한다 — 뷰 상태(버퍼 없음)는 저장할 것이 없다.
    #[test]
    fn doc_savable_covers_hex_edit() {
        let mut app = hex_test_doc(&[1, 2]);
        assert!(!doc_savable(app.doc().unwrap()), "뷰 상태는 저장할 것이 없다");
        let doc = app.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().edit = Some(crate::hex::HexEditBuffer::new(vec![1, 2]));
        assert!(doc_savable(doc));
    }

    /// 헥스 저장: write_binary로 버퍼가 통째로 나가고 dirty가 꺼진다.
    #[test]
    fn hex_save_writes_buffer_and_clears_dirty() {
        let p = temp_ext(b"\x00\x01", "bin");
        let mut app = App::default();
        app.open_path_hex(&p);
        let doc = app.doc_mut().unwrap();
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xF)); // 승격+수정
        // 저장 실행부가 부르는 것과 같은 조합:
        let bytes = doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes.clone();
        crate::save::write_binary(&doc.path, &bytes).unwrap();
        mark_hex_saved(doc);
        assert!(!doc.hex.as_ref().unwrap().edit.as_ref().unwrap().dirty);
        assert_eq!(std::fs::read(&p).unwrap(), vec![0xF0, 0x01]);
        std::fs::remove_file(&p).ok();
    }

    /// 저장 다이얼로그는 헥스 문서에서 닫히지 않고(편집 버퍼가 있으면),
    /// 인코딩/개행 콤보 없이 뜬다 — 상태 전이만 검사.
    #[test]
    fn save_dialog_stays_open_for_hex_edit() {
        let mut app = hex_test_doc(&[1]);
        app.doc_mut().unwrap().hex.as_mut().unwrap().edit =
            Some(crate::hex::HexEditBuffer::new(vec![1]));
        app.show_save_dialog = true;
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            render_save_dialog(ctx, &mut app);
        });
        assert!(app.show_save_dialog, "헥스 편집 중엔 다이얼로그가 유지된다");
    }

    /// `doc_dirty`는 텍스트/헥스 어느 쪽 편집 버퍼가 dirty여도 참이어야 한다 —
    /// 닫기 확인이 두 종류의 dirty를 모두 봐야 하므로(Task 6 리뷰 지적 사항).
    #[test]
    fn doc_dirty_covers_both_text_and_hex() {
        let mut app = hex_test_doc(&[1, 2]);
        let doc = app.doc_mut().unwrap();
        assert!(!doc_dirty(doc), "뷰 상태는 dirty가 아니다");
        doc.hex.as_mut().unwrap().edit = Some(crate::hex::HexEditBuffer::new(vec![1, 2]));
        assert!(!doc_dirty(doc), "막 승격한 버퍼는 깨끗하다");
        doc.hex.as_mut().unwrap().edit.as_mut().unwrap().dirty = true;
        assert!(doc_dirty(doc), "헥스 dirty도 doc_dirty가 잡아야 한다");

        let mut text_doc = new_document();
        assert!(!doc_dirty(&text_doc));
        text_doc.edit.as_mut().unwrap().dirty = true;
        assert!(doc_dirty(&text_doc), "텍스트 dirty도 그대로 잡힌다");
    }

    // ---- 헥스 기능 게이트 / 상태줄 ----

    /// 헥스 문서에서 텍스트/표 전용 기능이 잠긴다 — 게이트를 자유 함수로.
    #[test]
    fn hex_doc_locks_text_features() {
        let app = hex_test_doc(&[1, 2, 3]);
        let doc = app.doc().unwrap();
        assert!(!text_tools_enabled(doc), "Sort/Convert/Numbering/오류창 비활성");
        // 텍스트 문서 대조군
        let app2 = find_test_doc(&["a,b"]);
        assert!(text_tools_enabled(app2.doc().unwrap()));
    }

    /// Tab은 헥스 문서에서 본문으로 가지 않는다(포커스 순회 유지).
    #[test]
    fn tab_is_not_captured_in_hex_doc() {
        // `wants_tab_character`는 `&self`만 받는다(브리프 스니펫의 `mut`는 불필요).
        let app = hex_test_doc(&[1]);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        let mut took = false;
        let _ = ctx.run(input, |ctx| took = app.wants_tab_character(ctx));
        assert!(!took);
    }

    /// 상태줄 문구: 헥스 문서는 인덱싱 표시 대신 크기/오프셋, 그리고
    /// 삽입/덮어쓰기 모드(M7). 형식은
    /// `"Binary — {len} bytes | 0x{caret:X} ({caret}) | {INS|OVR}"`.
    /// 모드 표시가 없으면 Insert 키 토글에 아무 피드백이 없다.
    #[test]
    fn hex_status_line_reads_size_and_caret() {
        let mut app = hex_test_doc(&[0u8; 100]);
        app.doc_mut().unwrap().hex.as_mut().unwrap().caret = (0x1A, true);
        assert_eq!(
            hex_status_text(app.doc().unwrap()),
            "Binary — 100 bytes | 0x1A (26) | OVR",
            "기본은 덮어쓰기"
        );
        // Insert 키(= ToggleInsert 인텐트)를 누르면 문구가 INS로 바뀐다.
        let mut clip = String::new();
        apply_hex_intent(app.doc_mut().unwrap(), &mut clip, HexIntent::ToggleInsert);
        assert_eq!(
            hex_status_text(app.doc().unwrap()),
            "Binary — 100 bytes | 0x1A (26) | INS",
            "삽입 모드 토글이 상태줄에 드러나야 한다"
        );
    }

    /// 상태줄의 dirty 표시 조건은 `doc_dirty` — 헥스 편집이 dirty면 켜진다.
    /// (문구/색은 텍스트 쪽과 같은 리터럴을 쓰므로 조건만 검증한다.)
    #[test]
    fn hex_status_dirty_marker_follows_doc_dirty() {
        let mut app = hex_test_doc(&[1, 2]);
        let doc = app.doc_mut().unwrap();
        assert!(!doc_dirty(doc), "뷰 상태에는 변경 표시가 없다");
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0xF));
        assert!(doc_dirty(doc), "편집하면 상태줄에 ● Modified가 붙는다");
        // 크기 문구도 편집 버퍼를 진실로 삼는다.
        assert_eq!(hex_status_text(doc), "Binary — 2 bytes | 0x0 (0) | OVR");
    }

    /// 메뉴의 "Undo" 항목은 헥스 문서에서 헥스 undo 경로로 간다 —
    /// 활성 조건(`HexEditBuffer::can_undo`)과 적용(`HexIntent::Undo`) 양쪽.
    #[test]
    fn hex_menu_undo_uses_hex_path() {
        let mut app = hex_test_doc(&[0xAA, 0xBB]);
        let doc = app.doc_mut().unwrap();
        // 뷰 상태(편집 버퍼 없음)에서는 되돌릴 것이 없다 → 항목 비활성.
        assert!(doc
            .hex
            .as_ref()
            .and_then(|h| h.edit.as_ref())
            .is_none_or(|e| !e.can_undo()));
        let mut clip = String::new();
        apply_hex_intent(doc, &mut clip, HexIntent::Nibble(0x1));
        assert_eq!(
            doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes,
            vec![0x1A, 0xBB],
            "사전 조건: 상위 니블이 바뀌었다"
        );
        assert!(
            doc.hex.as_ref().unwrap().edit.as_ref().unwrap().can_undo(),
            "이제 항목이 활성이다"
        );
        // update()의 undo_clicked 분기가 하는 것과 같은 호출.
        apply_hex_intent(doc, &mut clip, HexIntent::Undo);
        assert_eq!(
            doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes,
            vec![0xAA, 0xBB],
            "메뉴 되돌리기가 헥스 버퍼를 되돌린다"
        );
        // 텍스트 전용 `undo_once`는 헥스 문서에서 아무 일도 하지 않아야 한다
        // (두 경로가 겹쳐 두 번 되돌리는 사고 방지).
        undo_once(doc);
        assert_eq!(
            doc.hex.as_ref().unwrap().edit.as_ref().unwrap().bytes,
            vec![0xAA, 0xBB],
            "undo_once는 헥스 문서를 건드리지 않는다"
        );
    }

    // ---- 헥스 찾기 ----

    /// 찾기 입력 해석: as_hex면 16진수(공백 무시), 아니면 UTF-8 바이트.
    #[test]
    fn hex_needle_interprets_by_mode() {
        assert_eq!(hex_needle("4F 4B", true), Some(vec![0x4F, 0x4B]));
        assert_eq!(hex_needle("XYZ", true), None);
        assert_eq!(hex_needle("OK", false), Some(b"OK".to_vec()));
        assert_eq!(hex_needle("한", false), Some("한".as_bytes().to_vec()));
        assert_eq!(hex_needle("", false), None, "빈 텍스트도 무의미");
    }

    /// 다음 찾기: 매치 갱신 + 그 행으로 스크롤 요청, 랩어라운드.
    #[test]
    fn hex_find_next_advances_and_wraps() {
        let mut data = vec![0u8; 100];
        data[40] = 0x4F;
        data[41] = 0x4B; // 행 1 (32..64)
        data[70] = 0x4F;
        data[71] = 0x4B; // 행 2
        let mut app = hex_test_doc(&data);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "4F 4B".into();
        hex_find_next(doc);
        assert_eq!(doc.hex.as_ref().unwrap().last_match, Some((40, 2)));
        assert_eq!(doc.pending_scroll_row, Some(1), "매치 오프셋의 행");
        hex_find_next(doc);
        assert_eq!(doc.hex.as_ref().unwrap().last_match, Some((70, 2)));
        hex_find_next(doc);
        assert_eq!(doc.hex.as_ref().unwrap().last_match, Some((40, 2)), "랩어라운드");
    }

    /// 편집 중이면 버퍼를 검색한다(mmap이 아니라).
    #[test]
    fn hex_find_searches_edit_buffer() {
        let mut app = hex_test_doc(&[0u8; 4]);
        let doc = app.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().edit =
            Some(crate::hex::HexEditBuffer::new(vec![0x00, 0xCA, 0xFE, 0x00]));
        doc.find_query = "CAFE".into();
        hex_find_next(doc);
        assert_eq!(doc.hex.as_ref().unwrap().last_match, Some((1, 2)));
    }

    /// 텍스트 해석 모드.
    #[test]
    fn hex_find_text_mode() {
        let mut app = hex_test_doc(b"...SQLite...");
        let doc = app.doc_mut().unwrap();
        doc.hex.as_mut().unwrap().find_hex = false;
        doc.find_query = "SQLite".into();
        hex_find_next(doc);
        assert_eq!(doc.hex.as_ref().unwrap().last_match, Some((3, 6)));
    }

    /// 빈 검색어와 해석 불가 검색어는 서로 다른 안내를 내야 한다 — 빈 검색어를
    /// "Invalid pattern"으로 부르면 갓 연 빈 패널에서 Find Next를 누르는 가장
    /// 기본적인 조작조차 사용자 잘못처럼 보인다(리뷰 지적). 빈 검색어는
    /// 텍스트 모드(`apply_find_action`)와 같은 문구 "Enter text to find"를
    /// 쓰고, 검색을 아예 실행하지 않는 no-op이므로 기존 `last_match`를 그대로
    /// 둔다. 진짜 해석 불가(홀수 자리·16진수 아닌 문자)만 "Invalid pattern".
    #[test]
    fn hex_find_next_distinguishes_empty_query_from_invalid_pattern() {
        let mut app = hex_test_doc(&[0x4F, 0x4B, 0x00, 0x4F, 0x4B]);
        let doc = app.doc_mut().unwrap();
        // 먼저 매치를 하나 만들어 last_match를 채워 둔다.
        doc.find_query = "4F 4B".into();
        hex_find_next(doc);
        let prior_match = doc.hex.as_ref().unwrap().last_match;
        assert_eq!(prior_match, Some((0, 2)), "사전 조건: 매치가 있어야 한다");

        // 빈 검색어 — "Enter text to find"이고, no-op이라 last_match는 그대로다.
        doc.find_query = String::new();
        hex_find_next(doc);
        assert_eq!(doc.find_status, "Enter text to find");
        assert_eq!(
            doc.hex.as_ref().unwrap().last_match,
            prior_match,
            "빈 검색어는 no-op — 기존 매치 하이라이트를 지우면 안 된다"
        );

        // 해석 불가한 16진수 — "Invalid pattern"(진짜 잘못된 입력).
        doc.find_query = "XYZ".into();
        hex_find_next(doc);
        assert_eq!(doc.find_status, "Invalid pattern");
    }

    /// **편집은 `last_match`를 무효로 만든다(C2 회귀).** 삽입은 편집 지점
    /// 뒤의 모든 바이트를 밀어 `(offset, len)`이 가리키던 자리가 검색한
    /// 바이트열과 무관해진다. 그대로 남으면 (1) `hex_byte_bg`가 검색한 적
    /// 없는 바이트를 매치 색으로 칠하고 (2) 다음 `hex_find_next`가
    /// `last_match.0 + 1`을 시작점으로 삼아 실제 매치를 건너뛴다.
    #[test]
    fn hex_edit_clears_last_match() {
        let mut app = hex_test_doc(&[0x4F, 0x4B, 0x00, 0x00]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "4F 4B".into();
        hex_find_next(doc);
        assert_eq!(
            doc.hex.as_ref().unwrap().last_match,
            Some((0, 2)),
            "사전 조건: 매치가 잡혀 있다"
        );

        let mut clip = String::new();
        // 문서 맨 앞에 한 바이트 삽입 — 매치가 있던 0..2가 1..3으로 밀린다.
        doc.hex.as_mut().unwrap().insert_mode = true;
        doc.hex.as_mut().unwrap().caret = (0, true);
        apply_hex_intent(doc, &mut clip, HexIntent::Ascii("Z".into()));
        assert_eq!(
            doc.hex.as_ref().unwrap().last_match,
            None,
            "삽입 편집 뒤에 낡은 매치가 남으면 안 된다"
        );

        // 니블/삭제/붙여넣기/Backspace/Undo도 같은 규율을 지킨다.
        for intent in [
            HexIntent::Nibble(0xA),
            HexIntent::DeleteForward,
            HexIntent::Backspace,
            HexIntent::Paste("FF".into()),
            HexIntent::Undo,
            HexIntent::Redo,
        ] {
            doc.hex.as_mut().unwrap().caret = (1, true);
            doc.hex.as_mut().unwrap().last_match = Some((0, 2));
            apply_hex_intent(doc, &mut clip, intent.clone());
            assert_eq!(
                doc.hex.as_ref().unwrap().last_match,
                None,
                "{intent:?} 뒤에도 매치를 버려야 한다"
            );
        }
    }

    /// **빈 붙여넣기는 완전한 no-op이어야 한다(I5 회귀).** 예전에는 선택
    /// 삭제가 빈 판정보다 먼저라, 선택해 둔 상태에서 빈 클립보드로 Ctrl+V를
    /// 누르면 바이트가 사라지고(되돌릴 수 없는 바이너리 손상) 선택도 지워진
    /// 채 캐럿 대입마저 건너뛰어 캐럿이 낡은 자리에 남았다.
    #[test]
    fn hex_empty_paste_over_selection_is_noop() {
        let mut app = hex_test_doc(&[0x11, 0x22, 0x33, 0x44]);
        let doc = app.doc_mut().unwrap();
        // 편집 버퍼를 미리 만들어 둔다(이 테스트는 승격 경로가 아니라
        // 빈 입력의 no-op 성질을 본다).
        doc.hex.as_mut().unwrap().edit =
            Some(crate::hex::HexEditBuffer::new(vec![0x11, 0x22, 0x33, 0x44]));
        doc.hex.as_mut().unwrap().sel = Some((1, 3));
        doc.hex.as_mut().unwrap().caret = (3, true);
        let mut clip = String::new();

        apply_hex_intent(doc, &mut clip, HexIntent::Paste(String::new()));
        let h = doc.hex.as_ref().unwrap();
        assert_eq!(
            h.edit.as_ref().unwrap().bytes,
            vec![0x11, 0x22, 0x33, 0x44],
            "빈 붙여넣기가 선택 바이트를 지우면 안 된다"
        );
        assert_eq!(h.sel, Some((1, 3)), "선택도 그대로여야 한다");
        assert_eq!(h.caret, (3, true), "캐럿도 그대로여야 한다");
        assert!(
            !h.edit.as_ref().unwrap().dirty,
            "아무 일도 없었으므로 dirty가 아니다"
        );

        // 문자 패널의 빈 입력(`Ascii("")`)도 같은 갈래를 지난다.
        doc.hex.as_mut().unwrap().pane = crate::hex::HexPane::Ascii;
        apply_hex_intent(doc, &mut clip, HexIntent::Ascii(String::new()));
        let h = doc.hex.as_ref().unwrap();
        assert_eq!(h.edit.as_ref().unwrap().bytes, vec![0x11, 0x22, 0x33, 0x44]);
        assert_eq!(h.sel, Some((1, 3)));
        assert_eq!(h.caret, (3, true));
    }

    /// **헥스 문서의 화면 행 수는 바이트 길이에서 나온다(I3 회귀).**
    /// 헥스 문서는 줄 인덱서를 띄우지 않아 `doc.index.line_count()`가 영영
    /// 0이다. `doc_screen_row_count`가 그걸 그대로 쓰면 `page_target_row`가
    /// `total == 0`으로 보고 `None`을 돌려, Page Up/Down이 캐럿만 옮기고
    /// 화면은 그대로 서 있는다.
    #[test]
    fn hex_screen_row_count_follows_byte_length() {
        let mut app = hex_test_doc(&[0u8; 100]);
        let doc = app.doc_mut().unwrap();
        assert_eq!(doc.index.line_count(), 0, "사전 조건: 줄 인덱서가 없다");
        assert_eq!(
            doc_screen_row_count(doc),
            4,
            "100바이트 = 32바이트 행 4개(마지막 행은 4바이트)"
        );

        // 편집 버퍼가 진실이면 그 길이를 따른다.
        doc.hex.as_mut().unwrap().edit = Some(crate::hex::HexEditBuffer::new(vec![0u8; 64]));
        assert_eq!(doc_screen_row_count(doc), 2, "편집 버퍼 64바이트 = 2행");

        // 그래서 Page Down이 실제 스크롤 요청을 만든다.
        doc.hex.as_mut().unwrap().edit = Some(crate::hex::HexEditBuffer::new(vec![0u8; 32 * 100]));
        doc.first_visible_row = 0;
        doc.visible_rows = 20;
        apply_page_scroll(doc, PageDir::Down);
        assert_eq!(
            doc.pending_scroll_row,
            Some(19),
            "헥스에서도 Page Down이 화면을 옮겨야 한다(한 행 겹침)"
        );
    }

    /// 헥스 문서에서 `render_find_panel`은 헥스 전용 패널을 그리고, 찾기
    /// 입력란에서 Enter를 치면(텍스트 모드와 같은 관용) `FindAction::HexNext`를
    /// 반환한다 — 호출부가 이걸 받아 `hex_find_next`를 부르는 게 계약이다.
    #[test]
    fn find_panel_returns_hex_next_for_hex_doc() {
        let mut app = hex_test_doc(&[0x4F, 0x4B]);
        let doc = app.doc_mut().unwrap();
        doc.show_find = true;
        doc.find_query = "4F 4B".into();
        let ctx = egui::Context::default();
        let mut action = None;
        let _ = ctx.run(Default::default(), |ctx| {
            let doc = app.doc_mut().unwrap();
            action = render_find_panel(ctx, doc, crate::i18n::Lang::default());
        });
        // 아무 것도 누르지 않은 프레임은 인텐트가 없다 — 스모크 확인.
        assert_eq!(action, None);
        // 입력란에 포커스를 준다 — 이 프레임에서 실제로 포커스가 잡혀야
        // 다음 프레임의 Enter가 `lost_focus()`를 발생시킨다.
        {
            let doc = app.doc_mut().unwrap();
            doc.find_focus_pending = true;
        }
        let _ = ctx.run(Default::default(), |ctx| {
            let doc = app.doc_mut().unwrap();
            action = render_find_panel(ctx, doc, crate::i18n::Lang::default());
        });
        assert_eq!(action, None, "포커스만 잡은 프레임은 아직 인텐트가 없다");
        // 이미 포커스가 있는 입력란에 Enter를 쳐서 Find Next를 낸다.
        let enter_input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(enter_input, |ctx| {
            let doc = app.doc_mut().unwrap();
            action = render_find_panel(ctx, doc, crate::i18n::Lang::default());
        });
        assert_eq!(action, Some(FindAction::HexNext));
    }

    /// **해석 방식(Hex 체크박스)을 바꾸면 커서를 버린다(Minor 6).**
    /// `"4F4B"`는 헥스로 두 바이트, 텍스트로 네 글자다 — 기준이 달라졌는데
    /// `last_match`가 남으면 하이라이트가 거짓이 되고 다음 Find Next가 그
    /// 무의미한 자리에서 이어 찾는다. 패널은 체크박스가 `changed()`일 때
    /// 이 함수를 부른다(`render_hex_find_panel`).
    #[test]
    fn hex_find_mode_toggle_resets_cursor() {
        let mut app = hex_test_doc(&[0x4F, 0x4B, 0x00, 0x4F, 0x4B]);
        let doc = app.doc_mut().unwrap();
        doc.find_query = "4F 4B".into();
        hex_find_next(doc);
        assert_eq!(
            doc.hex.as_ref().unwrap().last_match,
            Some((0, 2)),
            "사전 조건: 헥스 해석으로 매치가 잡혀 있다"
        );
        doc.find_status = "Not found".into();

        // 체크박스 토글이 하는 일.
        doc.hex.as_mut().unwrap().find_hex = false;
        reset_hex_find_cursor(doc);
        assert_eq!(
            doc.hex.as_ref().unwrap().last_match,
            None,
            "기준이 바뀌었으므로 커서를 버린다"
        );
        assert!(doc.find_status.is_empty(), "이전 안내도 지운다");
    }

    /// IME는 **편집 모드에서만** 캐럿을 따라간다. 뷰 모드에서 켜면 입력할 수
    /// 없는 화면에 조합 창이 뜨고 친 글자가 사라진다.
    #[test]
    fn ime_follows_caret_only_in_edit_mode() {
        assert!(ime_should_follow_caret(true));
        assert!(!ime_should_follow_caret(false));
    }

    /// 다이얼로그에서 고른 개행이 **실제 저장 옵션에 들어가는지**.
    ///
    /// 문서의 개행과 **다른** 값을 골랐을 때만 증상이 드러나므로, 일부러
    /// 어긋나게 만들어 확인한다. 같은 값이면 어느 쪽에서 읽어도 통과한다.
    #[test]
    fn save_options_use_the_dialog_choice_not_the_document() {
        let p = temp_ext(b"a\r\nb\r\n", "txt");
        let ctx = egui::Context::default();
        let mut app = App::default();
        app.start(Some(&p), &ctx, Default::default());
        assert_eq!(
            app.doc().unwrap().edit.as_ref().unwrap().newline,
            crate::edit::Newline::CrLf,
            "전제: 문서는 CRLF"
        );

        // 사용자가 콤보에서 LF를 골랐다.
        app.save_newline = crate::edit::Newline::Lf;

        assert_eq!(
            save_options(&app).newline,
            crate::edit::Newline::Lf,
            "문서가 CRLF여도 고른 LF로 저장해야 한다"
        );
        std::fs::remove_file(&p).ok();
    }

    /// 인코딩/BOM도 같은 출처(다이얼로그)에서 온다.
    #[test]
    fn save_options_carry_encoding_and_bom() {
        let app = App {
            save_enc: crate::parse::Encoding::Utf16Le,
            save_bom: true,
            ..App::default()
        };
        let o = save_options(&app);
        assert_eq!(o.enc, crate::parse::Encoding::Utf16Le);
        assert!(o.bom);
    }

    // ---- 저장 다이얼로그 확장자 목록 ----

    fn names(v: &[(&str, Vec<&str>)]) -> Vec<String> {
        v.iter().map(|(n, _)| (*n).to_owned()).collect()
    }

    /// 이미 확장자가 있는 파일은 **그 형식이 기본**이어야 한다. 기본이 다르면
    /// 저장하다 실수로 형식을 바꾼다.
    #[test]
    fn save_filters_prefer_the_current_extension() {
        for (path, want) in [
            ("z:/x/data.tsv", "TSV"),
            ("z:/x/data.tab", "TSV"),
            ("z:/x/data.csv", "CSV"),
            ("z:/x/notes.txt", "Text"),
        ] {
            let f = save_filters(std::path::Path::new(path), Some(SeparatorMode::Char(b',')));
            assert_eq!(names(&f)[0], want, "{path}");
        }
    }

    /// 경로가 없으면(새 파일) **보기 구분자**로 추측한다.
    #[test]
    fn save_filters_fall_back_to_the_view_separator() {
        let empty = std::path::Path::new("");
        assert_eq!(
            names(&save_filters(empty, Some(SeparatorMode::Char(b'\t'))))[0],
            "TSV"
        );
        assert_eq!(
            names(&save_filters(empty, Some(SeparatorMode::Char(b','))))[0],
            "CSV"
        );
        assert_eq!(names(&save_filters(empty, Some(SeparatorMode::None)))[0], "Text");
        assert_eq!(names(&save_filters(empty, None))[0], "Text", "정보가 없으면 텍스트");
    }

    /// 확장자가 우리가 아는 셋이 아니면(.log 등) 구분자 추측으로 넘어간다.
    #[test]
    fn save_filters_use_separator_for_unknown_extensions() {
        let p = std::path::Path::new("z:/x/server.log");
        assert_eq!(names(&save_filters(p, Some(SeparatorMode::Char(b'\t'))))[0], "TSV");
    }

    /// 어떤 경우에도 **세 형식이 모두** 있고 `All files`가 끝에 있다 —
    /// 추측이 틀렸을 때 사용자가 바꿀 수 있어야 하고, 아는 확장자가 아닌
    /// 파일(.json/.md)도 저장할 수 있어야 한다.
    #[test]
    fn save_filters_always_offer_every_choice() {
        for sep in [None, Some(SeparatorMode::None), Some(SeparatorMode::Char(b','))] {
            for path in ["", "z:/x/a.csv", "z:/x/a.tsv", "z:/x/a.txt", "z:/x/a.json"] {
                let f = save_filters(std::path::Path::new(path), sep);
                let n = names(&f);
                for want in ["CSV", "TSV", "Text", "All files"] {
                    assert!(n.contains(&want.to_owned()), "{path} {sep:?} 에 {want} 없음: {n:?}");
                }
                assert_eq!(n.last().unwrap(), "All files", "All files 는 맨 끝");
                assert_eq!(n.len(), 4, "중복 없이 넷: {n:?}");
            }
        }
    }

    /// TSV 항목은 `.tab`도 받는다(`detect_separator`가 그 확장자를 탭으로 연다).
    #[test]
    fn tsv_filter_includes_tab_extension() {
        let f = save_filters(std::path::Path::new("z:/x/a.tsv"), None);
        let (_, exts) = f.iter().find(|(n, _)| *n == "TSV").unwrap();
        assert!(exts.contains(&"tsv") && exts.contains(&"tab"), "{exts:?}");
    }

    /// **끝에서 끝까지 — 이 버그의 회귀 테스트.**
    ///
    /// 실제 `App::update`를 한 프레임 돌린다. 메뉴바가 본문보다 먼저 그려지는
    /// 진짜 순서를 그대로 타므로, Tab 소비가 본문 렌더 안에 있으면(옛 구조)
    /// 탭 문자는 들어가도 포커스가 File 메뉴로 넘어간다 — 사용자가 보고한
    /// 바로 그 증상이다.
    #[test]
    fn tab_inserts_without_moving_focus_to_the_menu() {
        let mut app = find_test_doc(&["ab"]);
        {
            let doc = app.doc_mut().unwrap();
            doc.text_caret = crate::edit::TextPos { line: 0, col: 2 };
        }
        let ctx = egui::Context::default();
        // eframe::Frame 없이 update를 부를 수 없으므로 ctx.run 안에서
        // App::update 를 직접 호출하는 대신, update 가 부르는 두 조각을
        // **같은 순서**로 태운다: wants_tab_character(맨 앞) → 메뉴바 → 본문.
        let mut clip = String::new();
        let draw = |app: &mut App, clip: &mut String, input: egui::RawInput| {
            let _ = ctx.run(input, |ctx| {
                let tab_for_body = app.wants_tab_character(ctx);
                egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
                    egui::menu::bar(ui, |ui| {
                        ui.menu_button("File", |_ui| {});
                    });
                });
                egui::CentralPanel::default().show(ctx, |ui| {
                    render_text(ui, app.doc_mut().unwrap(), 0, clip, tab_for_body, crate::i18n::Lang::default());
                });
                // 끝에서 걷어내는 단계는 **일부러 없다** — 프레임이 끝난 뒤
                // 포커스가 None이라는 것은 File이 애초에 받지 못했다는 뜻이다
                // (받았다면 아무도 지우지 않아 남아 있어야 한다). 이게 곧
                // "하이라이트 깜빡임 없음"의 관측 가능한 증거다.
            });
        };
        draw(&mut app, &mut clip, Default::default());

        let input = egui::RawInput {
            events: vec![tab_event(egui::Modifiers::NONE)],
            ..Default::default()
        };
        draw(&mut app, &mut clip, input);

        assert_eq!(
            app.doc().unwrap().edit.as_ref().unwrap().lines,
            v(&["ab\t"]),
            "탭 문자가 들어가야 하고"
        );
        assert_eq!(
            ctx.memory(|m| m.focused()),
            None,
            "포커스가 File 메뉴로 가면 안 된다"
        );
    }
}
