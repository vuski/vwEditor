# vwEditor

**한국어** | [English](#vweditor-english)

> 이 저장소는 [vuski/vwEditor](https://github.com/vuski/vwEditor)의 포크입니다.
> 원본 대비 **가로 스크롤**, **컬럼별 필터링**, **zip 안 CSV/TSV 열기(단일 파일)**를
> 추가했습니다. 아래 설명은 원본 README를 그대로 두고 있습니다.

대용량 CSV/TSV/텍스트 파일을 즉시 여는 뷰어 겸 에디터입니다. Rust + egui.
parquet 조회도 가능합니다.

10GB 파일을 열어도 첫 화면이 곧바로 뜨도록 했습니다. 파일 전체를 읽지 않고 앞부분만 훑어 표시한 뒤, 나머지 줄 위치는 백그라운드에서 인덱싱합니다.
스크롤은 읽은 데까지 따라 늘어납니다.

> ### ⚠️ 백신 오탐 안내
>
> Windows Defender가 이 프로그램을 **`Trojan:Win32/Wacatac.B!ml`** 로
> 잡을 수 있습니다. **오탐이며 Microsoft에 신고했습니다.**
>
> 이름 끝의 `!ml`은 "머신러닝 추측"이라는 뜻입니다 — 알려진 악성코드와
> 일치해서가 아니라, 모델이 수상하다고 판단했다는 표시입니다. 서명 없는
> 정적 링크 Rust 바이너리(+LTO, mimalloc)가 패킹된 실행 파일과 겉모양이
> 겹쳐서 생기는 흔한 오탐입니다.
>
> 이 프로그램은 **네트워크 기능이 아예 없습니다** — HTTP 클라이언트도,
> 소켓도, 네트워크 크레이트도 의존성에 없습니다. 레지스트리를 건드리지
> 않고, 다른 프로세스를 띄우지 않으며, 설치나 자동 실행 등록도 하지
> 않습니다. 사용자가 직접 열거나 저장하는 파일만 읽고 씁니다.
>
> 직접 확인하시려면: 소스가 전부 공개되어 있으니 `cargo build --release`로
> 직접 빌드하시거나, [VirusTotal](https://www.virustotal.com)에 올려
> 다른 백신들의 판정을 비교해 보세요.

> 개인 도구로 만들어 쓰는 중입니다. Windows에서 실사용하며 다듬었고,
> 831개 테스트가 붙어 있습니다. macOS·Linux는 빌드는 되지만
> **실기 검증을 못 했습니다** — 아래 [플랫폼 지원](#플랫폼-지원)을 참고하세요.

## 기능

- **대용량 파일** — mmap + 백그라운드 줄 인덱싱. 10.8GB / 3억 행 정렬 5.9초(실측).
- **표 모드 / 텍스트 모드** — 구분자를 감지해 표로 보여주거나, 원문 그대로 봅니다.
- **자동 감지** — 인코딩(BOM → UTF-8 → CP949), 구분자, 헤더 유무. 전부 수동으로 덮어쓸 수 있습니다.
- **찾기·바꾸기** — 대소문자·전체 셀 일치 옵션. 전체 바꾸기는 899MB / 15.4M 행에서 약 0.25초.
- **추출** - 특정 문자열이 포함된 행만 추출할 수 있습니다.
- **정렬** — 다중 키 정렬. 문자열/숫자 판별.
- **편집·저장** — 인코딩·개행(CRLF/LF)·BOM을 골라 저장합니다. Undo/Redo.
- **Hex 모드** — 바이너리 파일을 16진수로 봅니다(읽기 전용).
- **Parquet / GeoParquet** — 읽기 전용. geometry 컬럼은 `POINT(127.02 37.51)`,
  `POLYGON(1,204 pts)` 형태로 요약해 보여줍니다. CSV/TSV로 내보내기 가능합니다.
- **zip 안 CSV/TSV 열기** — csv/tsv/psv/txt 파일 하나짜리 zip을 그대로 열 수 있습니다
  (읽기 전용, 저장은 다른 이름으로).
- **파싱 오류 행 검출** — 따옴표가 안 닫힌 행 등을 찾아 줍니다.
- **한국어·영어 UI** — 시작할 때 OS 로케일을 따르고, 메뉴 > 언어에서 바꿀 수 있습니다.
- **멀티탭**, **드래그앤드롭**, **Ctrl+휠 확대**(0.5~4.0배).

## 빌드

Rust 툴체인이 필요합니다([rustup](https://rustup.rs/)).

```powershell
cargo build --release
```

결과물은 `target/release/vweditor.exe` 하나입니다. 별도 런타임이 필요 없습니다.

### Linux 빌드 의존성

GUI 라이브러리 개발 패키지가 필요합니다.

```bash
# Debian / Ubuntu
sudo apt install build-essential libgtk-3-dev libxkbcommon-dev

# Fedora
sudo dnf install gtk3-devel libxkbcommon-devel
```

## 플랫폼 지원

**Windows에서 만들었고 Windows에서만 실사용 검증했습니다.** 나머지 둘은
코드상 크로스 플랫폼이지만 실제로 돌려 보지 못했습니다.

|         | 빌드 | 실기 검증 | 비고                                  |
| ------- | ---- | --------- | ------------------------------------- |
| Windows | ✅   | ✅        | 개발·상용 환경                        |
| macOS   | ✅   | ❌        | 단축키가 Ctrl (맥 관례는 Cmd)         |
| Linux   | ✅   | ❌        | 개발 패키지 필요, 한글 폰트 확인 필요 |

의존성은 전부 크로스 플랫폼입니다(eframe/egui, memmap2, rfd, rayon, parquet, mimalloc). Windows 전용 크레이트는 쓰지 않습니다.

### 폰트에 대해

**폰트를 앱에 내장하지 않습니다.** 실행 파일과 저장소를 가볍게 유지하기
위해서입니다. 대신 시스템 폰트를 찾아 씁니다 —
[src/theme.rs](src/theme.rs)의 `MONO_CANDIDATES` / `UI_CANDIDATES` /
`KR_CANDIDATES`에 플랫폼별 경로 목록이 있습니다.

**한글 폰트가 없으면 한글이 □로 보입니다.** egui 내장 폰트에는 CJK 글리프가
없기 때문입니다. UI를 한국어로 쓸 때는 물론이고, **UI가 영어여도 파일 안의
한글 데이터가 □가 됩니다** — 이쪽이 실제로는 더 자주 부딪히는 경우입니다.
이 경우 앱이 시작할 때 설치 방법을 안내합니다(그 안내만은 영어입니다 —
한글을 못 그린다는 안내가 한글이면 읽을 수 없으니까요).

```bash
# Debian / Ubuntu
sudo apt install fonts-nanum

# Fedora
sudo dnf install nhn-nanum-fonts

# Arch
sudo pacman -S noto-fonts-cjk
```

설치했는데도 □가 보이면 그 폰트 경로가 목록에 없는 경우입니다.
`KR_CANDIDATES`에 경로를 추가하면 됩니다. — 배포판마다
경로가 달라서 목록이 아직 성깁니다.

### macOS 사용자에게

단축키가 `Ctrl` 기준이라 맥 관례(`Cmd`)와 어긋납니다. 맥에서도 Ctrl 키는 동작하므로 못 쓸 정도는 아니지만 어색합니다. egui의 `Modifiers::command`는 플랫폼별로 Ctrl/Cmd를 자동 매핑하므로, 현재 `ctrl` 검사를 그것으로 바꾸면 양쪽이 동시에 맞습니다.

새 파일 기본 개행이 CRLF인 점도 맥·리눅스에서는 어색할 수 있습니다. 저장 다이얼로그에서 LF로 바꿀 수 있습니다.

## 기여

맥·리눅스 쪽은 위에 적은 대로 손댈 곳이 남아 있고, 해당 환경에서 검증하지 못했습니다.

- 코드 주석과 문서는 한국어입니다.
- 테스트를 붙여 주면 좋습니다 (`cargo test`).
- `.readme/` 에 작업 기록이 날짜순으로 쌓여 있습니다. 어떤 판단으로 지금 구조가 나왔는지 대부분 거기 적혀 있습니다.

## 개발

```powershell
cargo test              # 831개
cargo clippy --all-targets
cargo build --release
```

## 라이선스

[GNU General Public License v3.0](LICENSE).

이 프로그램을 수정해 배포하면 그 소스도 같은 GPL v3으로 공개해야 합니다.
개인적으로 고쳐 쓰는 것은 자유입니다.

---

# vwEditor (English)

[한국어](#vweditor) | **English**

> This repository is a fork of [vuski/vwEditor](https://github.com/vuski/vwEditor).
> It adds **horizontal scrolling**, **per-column filtering**, and **opening a
> single CSV/TSV file inside a zip** on top of the original. The description
> below is kept as-is from the original README.

A viewer and editor that opens large CSV/TSV/text files instantly. Rust + egui.
It reads parquet files too.

A 10GB file shows its first screen immediately. Instead of reading the whole
file, it scans just the beginning and displays that, then indexes the remaining
line positions in the background. The scrollbar grows as indexing progresses.

> ### ⚠️ Antivirus false positive
>
> Windows Defender may flag this program as **`Trojan:Win32/Wacatac.B!ml`**.
> **This is a false positive and has been reported to Microsoft.**
>
> The `!ml` suffix means the verdict comes from a machine-learning guess, not
> from a match against known malware. Unsigned, statically linked Rust binaries
> built with LTO and a replaced global allocator (mimalloc) resemble packed
> executables to the heuristic model — a common false positive.
>
> This program has **no networking whatsoever** — no HTTP client, no sockets,
> and no network crate anywhere in its dependency tree. It does not touch the
> registry, spawn processes, or install anything. It only reads and writes the
> files you explicitly open or save.
>
> Verify it yourself: the full source is public, so you can build it with
> `cargo build --release`, or upload the binary to
> [VirusTotal](https://www.virustotal.com) and compare verdicts across engines.

> This is a personal tool I built for my own use. It has been refined through
> daily use on Windows and carries 831 tests. macOS and Linux builds compile
> but **have not been verified on real hardware** — see
> [Platform Support](#platform-support) below.

## Features

- **Large files** — mmap + background line indexing. Sorting 10.8GB / 300M rows takes 5.9s (measured).
- **Table mode / text mode** — detects the delimiter and shows a table, or shows the raw text.
- **Auto-detection** — encoding (BOM → UTF-8 → CP949), delimiter, header presence. All can be overridden manually.
- **Find & replace** — case-sensitivity and whole-cell match options. Replace-all takes about 0.25s on 899MB / 15.4M rows.
- **Extract** — pull out only the rows containing a given string.
- **Sort** — multi-key sorting with string/numeric detection.
- **Edit & save** — choose encoding, line ending (CRLF/LF), and BOM when saving. Undo/redo.
- **Hex mode** — view binary files as hexadecimal (read-only).
- **Parquet / GeoParquet** — read-only. Geometry columns are summarized as
  `POINT(127.02 37.51)` or `POLYGON(1,204 pts)`. Exportable to CSV/TSV.
- **Open CSV/TSV inside a zip** — opens a zip containing a single csv/tsv/psv/txt file
  directly (read-only; save goes to a new path).
- **Malformed row detection** — finds rows with unclosed quotes and similar problems.
- **English and Korean UI** — follows your OS locale at startup; switchable under **Language** in the menu bar.
- **Multiple tabs**, **drag and drop**, **Ctrl+wheel zoom** (0.5×–4.0×).

## Build

You need the Rust toolchain ([rustup](https://rustup.rs/)).

```powershell
cargo build --release
```

The output is a single `target/release/vweditor.exe`. No separate runtime is required.

### Linux build dependencies

GUI library development packages are required.

```bash
# Debian / Ubuntu
sudo apt install build-essential libgtk-3-dev libxkbcommon-dev

# Fedora
sudo dnf install gtk3-devel libxkbcommon-devel
```

## Platform Support

**This was built on Windows and has only been verified in real use on Windows.**
The other two are cross-platform in the code but have never actually been run.

|         | Builds | Verified on real hardware | Notes                                            |
| ------- | ------ | ------------------------- | ------------------------------------------------ |
| Windows | ✅     | ✅                        | Development and daily-use environment            |
| macOS   | ✅     | ❌                        | Shortcuts use Ctrl (macOS convention is Cmd)     |
| Linux   | ✅     | ❌                        | Needs dev packages; Korean font needs checking   |

All dependencies are cross-platform (eframe/egui, memmap2, rfd, rayon, parquet,
mimalloc). No Windows-only crates are used.

### About fonts

**Fonts are not bundled with the app.** This keeps the executable and the
repository small. Instead the app looks for system fonts — see
`MONO_CANDIDATES` / `UI_CANDIDATES` / `KR_CANDIDATES` in
[src/theme.rs](src/theme.rs) for the per-platform path lists.

**Without a Korean font, Korean text renders as □.** egui's built-in fonts
contain no CJK glyphs. This affects the Korean UI, but also — more commonly —
**Korean data inside your files, even when the UI is in English.** When this
happens the app shows installation instructions at startup (that one message is
always in English, since a notice about missing Korean glyphs would be unreadable
in Korean).

```bash
# Debian / Ubuntu
sudo apt install fonts-nanum

# Fedora
sudo dnf install nhn-nanum-fonts

# Arch
sudo pacman -S noto-fonts-cjk
```

If you still see □ after installing a font, its path is missing from the list.
Adding the path to `KR_CANDIDATES` fixes it — the list is still sparse because
paths differ across distributions.

### For macOS users

Shortcuts are Ctrl-based, which conflicts with the macOS convention of Cmd. The
Ctrl key does work on macOS, so it is usable but awkward. egui's
`Modifiers::command` maps to Ctrl or Cmd automatically per platform, so
switching the current `ctrl` checks to that would make both correct at once.

The default line ending for new files is CRLF, which may also feel wrong on
macOS and Linux. You can change it to LF in the save dialog.

## Contributing

The macOS and Linux side has the rough edges described above, and I have not
been able to verify them in those environments.

- Code comments and documentation are in Korean.
- Tests are appreciated (`cargo test`).
- `.readme/` holds work logs in date order. Most of the reasoning behind the
  current structure is written there.

## Development

```powershell
cargo test              # 831 tests
cargo clippy --all-targets
cargo build --release
```

## License

[GNU General Public License v3.0](LICENSE).

If you modify and distribute this program, you must release your source under
the same GPL v3. Modifying it for your own private use is free.
