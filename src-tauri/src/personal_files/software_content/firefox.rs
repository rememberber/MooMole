//! Firefox Places download annotations, never ordinary browsing URLs or the
//! session download store. Query only a validated private database/WAL copy.
use super::*;

// The source implements DownloadHistory's destinationFileURI + metaData and
// the history query's TRANSITION_DOWNLOAD (7). Aggregation refuses ambiguous
// destination or metadata annotations instead of choosing one arbitrarily.
pub(super) const QUERY: &str = "
WITH destination AS (
 SELECT a.place_id, MAX(a.id) AS id,
 CASE WHEN COUNT(*)=1 THEN MAX(CASE WHEN typeof(a.content)='text'
  AND length(CAST(a.content AS BLOB))<=16384 THEN a.content END) END AS uri
 FROM moz_annos a JOIN moz_anno_attributes t ON t.id=a.anno_attribute_id
 WHERE t.name='downloads/destinationFileURI' GROUP BY a.place_id
), metadata AS (
 SELECT a.place_id,
 CASE WHEN COUNT(*)=1 THEN MAX(CASE WHEN typeof(a.content)='text'
  AND length(CAST(a.content AS BLOB))<=16384 THEN a.content END) END AS content
 FROM moz_annos a JOIN moz_anno_attributes t ON t.id=a.anno_attribute_id
 WHERE t.name='downloads/metaData' GROUP BY a.place_id
)
SELECT d.id,d.uri,m.content FROM destination d
 JOIN moz_places p ON p.id=d.place_id
 LEFT JOIN metadata m ON m.place_id=d.place_id
 WHERE EXISTS(SELECT 1 FROM moz_historyvisits v WHERE v.place_id=p.id AND v.visit_type=7)
 ORDER BY d.id DESC LIMIT 2001";

#[derive(Deserialize)]
struct Metadata {
    state: i64,
    #[serde(default)]
    deleted: bool,
}
pub(super) fn path(uri: &str, metadata: Option<&str>) -> Result<Option<String>, ()> {
    let value: Metadata = serde_json::from_str(metadata.ok_or(())?).map_err(|_| ())?;
    if value.state != 1 || value.deleted {
        return Ok(None);
    }
    if uri.len() > 16384 || uri.chars().any(char::is_control) {
        return Err(());
    }
    // Do not let URL normalization silently accept broken escape sequences.
    let b = uri.as_bytes();
    for (i, c) in b.iter().enumerate() {
        if *c == b'%'
            && (b.get(i + 1).is_none_or(|c| !c.is_ascii_hexdigit())
                || b.get(i + 2).is_none_or(|c| !c.is_ascii_hexdigit()))
        {
            return Err(());
        }
    }
    let url = reqwest::Url::parse(uri).map_err(|_| ())?;
    if url.scheme() != "file"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_some_and(|h| h != "localhost")
    {
        return Err(());
    }
    let file = url.to_file_path().map_err(|_| ())?;
    let path = file.to_str().ok_or(())?;
    if path.len() > 4096 || path.chars().any(char::is_control) || !file.is_absolute() {
        return Err(());
    }
    Ok(Some(path.into()))
}

#[cfg(test)]
pub(in crate::personal_files) fn fixture(history: &Path, selected: &Path) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(history).unwrap();
    db.pragma_update(None, "journal_mode", "WAL").unwrap();
    db.execute_batch("CREATE TABLE moz_places(id INTEGER PRIMARY KEY,url TEXT);
      CREATE TABLE moz_historyvisits(id INTEGER PRIMARY KEY,place_id INTEGER,visit_type INTEGER);
      CREATE TABLE moz_anno_attributes(id INTEGER PRIMARY KEY,name TEXT UNIQUE);
      CREATE TABLE moz_annos(id INTEGER PRIMARY KEY,place_id INTEGER,anno_attribute_id INTEGER,content TEXT);
      INSERT INTO moz_anno_attributes VALUES(1,'downloads/destinationFileURI'),(2,'downloads/metaData');
      INSERT INTO moz_places VALUES(1,'https://synthetic.invalid/owned-download');
      INSERT INTO moz_historyvisits VALUES(1,1,7);
      INSERT INTO moz_annos VALUES(2,1,2,'{\"state\":1,\"deleted\":false,\"fileSize\":1024,\"endTime\":1790000000000}');").unwrap();
    db.execute(
        "INSERT INTO moz_annos VALUES(1,1,1,?1)",
        [reqwest::Url::from_file_path(selected).unwrap().to_string()],
    )
    .unwrap();
    db
}

#[cfg(test)]
mod tests;
