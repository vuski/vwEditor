use std::io::Read;
use std::path::Path;

/// zip 안에서 꺼낸 표 형식 파일 하나.
#[derive(Debug)]
pub struct ZipTableEntry {
    /// zip 안의 원래 경로(예: `data/sales.csv`). 구분자 감지에 확장자를 쓴다.
    pub name: String,
    pub bytes: Vec<u8>,
}

const TABLE_EXTS: [&str; 4] = ["csv", "tsv", "psv", "txt"];

fn is_table_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| TABLE_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// zip 로컬 파일 헤더 매직(`PK\x03\x04`) 또는 빈 아카이브의 EOCD만(`PK\x05\x06`)으로
/// zip 여부를 판단한다. `open_path`가 이미 헤더 바이트를 읽어 뒀으므로(Parquet의
/// `PAR1` 검사와 같은 자리) 그 바이트를 그대로 재사용한다 — 파일을 다시 열지 않는다.
pub fn is_zip(head: &[u8]) -> bool {
    head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06")
}

/// zip 안에서 표 형식으로 보이는 파일(csv/tsv/psv/txt, 디렉터리 제외) 하나를 찾아
/// 통째로 압축을 풀어 온다.
///
/// 후보가 0개거나 2개 이상이면 사람이 읽을 에러 문자열로 실패한다 — 여러 파일 중
/// 하나를 고르는 선택 UI는 아직 없다(첫 버전 범위 밖. 필요해지면 여기 대신
/// `list_table_entries` + 선택 다이얼로그를 추가한다).
pub fn extract_single_table(path: &Path) -> Result<ZipTableEntry, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {e}"))?;

    let mut candidate: Option<usize> = None;
    let mut count = 0usize;
    for i in 0..archive.len() {
        let f = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {e}"))?;
        if f.is_file() && is_table_name(f.name()) {
            count += 1;
            candidate = Some(i);
        }
    }

    match count {
        0 => Err("No CSV/TSV file found inside the zip archive.".to_owned()),
        1 => {
            let idx = candidate.expect("count == 1 means candidate was set");
            let mut f = archive
                .by_index(idx)
                .map_err(|e| format!("Failed to read zip entry: {e}"))?;
            let name = f.name().to_owned();
            let mut bytes = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut bytes)
                .map_err(|e| format!("Failed to decompress zip entry: {e}"))?;
            Ok(ZipTableEntry { name, bytes })
        }
        n => Err(format!(
            "The zip archive contains {n} CSV/TSV files; only single-file archives are supported."
        )),
    }
}

/// 다른 모듈(`app.rs`의 통합 테스트)에서도 zip 픽스처가 필요해 공개해 둔다.
/// `parquet::testutil`과 같은 패턴 — 실제 파일 포맷 라이터는 여기 한 곳에만
/// 있어야 테스트마다 포맷 세부사항이 갈라지지 않는다.
#[cfg(test)]
pub mod testutil {
    use std::io::Write;

    /// 테스트마다 고유한 임시 zip 경로. `source.rs`의 `temp_file_with`와 같은
    /// 규율 — 내용으로 이름을 지으면 같은 내용을 쓰는 병렬 테스트끼리 경합한다.
    pub fn temp_path(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("tv_zip_{}_{}_{}.zip", std::process::id(), tag, id));
        p
    }

    /// `entries`를 담은 deflate 압축 zip을 `path`에 쓴다.
    pub fn write_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(content).unwrap();
        }
        w.finish().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_zip(entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = testutil::temp_path("unit");
        testutil::write_zip(&path, entries);
        path
    }

    #[test]
    fn detects_zip_magic() {
        assert!(is_zip(b"PK\x03\x04rest"));
        assert!(!is_zip(b"a,b,c\n1,2,3\n"));
        assert!(!is_zip(b"PAR1garbage"));
    }

    #[test]
    fn extracts_the_single_csv_entry() {
        let p = write_zip(&[("data.csv", b"a,b\n1,2\n")]);
        let entry = extract_single_table(&p).unwrap();
        assert_eq!(entry.name, "data.csv");
        assert_eq!(entry.bytes, b"a,b\n1,2\n");
    }

    #[test]
    fn ignores_non_table_entries() {
        let p = write_zip(&[("readme.md", b"note"), ("data.csv", b"a,b\n1,2\n")]);
        let entry = extract_single_table(&p).unwrap();
        assert_eq!(entry.name, "data.csv");
    }

    #[test]
    fn errs_when_no_table_entry_found() {
        let p = write_zip(&[("readme.md", b"hello")]);
        assert!(extract_single_table(&p).is_err());
    }

    #[test]
    fn errs_when_multiple_table_entries_found() {
        let p = write_zip(&[("a.csv", b"1"), ("b.csv", b"2")]);
        let err = extract_single_table(&p).unwrap_err();
        assert!(err.contains('2'));
    }
}
