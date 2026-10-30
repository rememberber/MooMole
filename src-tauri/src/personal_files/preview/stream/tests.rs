use super::*;
fn wav() -> Vec<u8> {
    let samples = 16_000u32;
    let mut v = b"RIFF".to_vec();
    v.extend((36 + samples * 2).to_le_bytes());
    v.extend(b"WAVEfmt ");
    v.extend(16u32.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(8000u32.to_le_bytes());
    v.extend(16000u32.to_le_bytes());
    v.extend(2u16.to_le_bytes());
    v.extend(16u16.to_le_bytes());
    v.extend(b"data");
    v.extend((samples * 2).to_le_bytes());
    v.resize(v.len() + samples as usize * 2, 0);
    v
}
fn item(path: &Path) -> Item {
    let witness = crate::observation::witness(path).unwrap();
    Item {
        id: uuid::Uuid::new_v4().to_string(),
        path: path.to_str().unwrap().into(),
        size: witness.size,
        modified_at: 0,
        witness,
    }
}
fn lease(path: &Path) -> Lease {
    let item = item(path);
    let prepared = prepare(&item, &[]).unwrap();
    Lease {
        id: uuid::Uuid::new_v4().to_string(),
        domain: Domain::PersonalFiles,
        scan_id: uuid::Uuid::new_v4().to_string(),
        item,
        source: None,
        prepared,
        deadline: Instant::now() + Duration::from_secs(60),
        expires_at: db::now() + 60_000,
    }
}
#[test]
fn real_owned_audio_ranges_head_and_seek_return_only_the_selected_bytes_without_modification() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().canonicalize().unwrap().join("owned.wav");
    let bytes = wav();
    std::fs::write(&path, &bytes).unwrap();
    let lease = lease(&path);
    for (request, expected) in [
        ("bytes=0-43", bytes[..44].to_vec()),
        ("bytes=1000-1999", bytes[1000..2000].to_vec()),
        ("bytes=-100", bytes[bytes.len() - 100..].to_vec()),
    ] {
        let req = Request::builder()
            .header(header::RANGE, request)
            .body(vec![])
            .unwrap();
        let res = response(&lease, &req, "tauri://localhost", || file_gate(&lease, &[]));
        assert_eq!(res.status(), 206);
        assert_eq!(res.body(), &expected);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "audio/wav");
    }
    let head = Request::builder().method("HEAD").body(vec![]).unwrap();
    let res = response(&lease, &head, "tauri://localhost", || {
        file_gate(&lease, &[])
    });
    assert!(res.body().is_empty());
    assert_eq!(
        res.headers()[header::CONTENT_LENGTH],
        bytes.len().to_string()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
#[test]
fn actual_generated_h264_video_and_large_sparse_pcm_seek_without_loading_whole_files() {
    let t = tempfile::Builder::new()
        .prefix("MooMole-owned-media-")
        .tempdir()
        .unwrap();
    let root = t.path().canonicalize().unwrap();
    let movie = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/media/owned-color.mp4"),
    )
    .unwrap();
    let path = root.join("owned.mp4");
    std::fs::write(&path, &movie).unwrap();
    let video = lease(&path);
    assert_eq!(video.prepared.kind, "video");
    let request = Request::builder()
        .header(header::RANGE, "bytes=0-1023")
        .body(vec![])
        .unwrap();
    let res = response(&video, &request, "tauri://localhost", || {
        file_gate(&video, &[])
    });
    assert_eq!(res.status(), 206);
    assert_eq!(res.body(), &movie[..1024]);
    assert_eq!(std::fs::read(&path).unwrap(), movie);
    let path = root.join("large.wav");
    let mut header = wav()[..44].to_vec();
    let payload = 64u32 * 1024 * 1024;
    header[4..8].copy_from_slice(&(payload + 36).to_le_bytes());
    header[40..44].copy_from_slice(&payload.to_le_bytes());
    std::fs::write(&path, &header).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(payload as u64 + 44)
        .unwrap();
    let audio = lease(&path);
    let request = Request::builder()
        .header(header::RANGE, "bytes=16777216-")
        .body(vec![])
        .unwrap();
    let res = response(&audio, &request, "tauri://localhost", || {
        file_gate(&audio, &[])
    });
    assert_eq!(res.status(), 206);
    assert_eq!(res.body().len(), CHUNK as usize);
    assert!(res.body().iter().all(|b| *b == 0));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), payload as u64 + 44);
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/media-preview-native.json",serde_json::to_vec_pretty(&serde_json::json!({"realOwnedH264Bytes":movie.len(),"videoRangeVerified":true,"largePcmBytes":audio.item.size,"maximumResponseBytes":res.body().len(),"sourceFilesRetained":true,"playbackWindowNotVerified":true})).unwrap()).unwrap();
}
#[test]
fn revoked_expired_protected_and_changed_files_never_return_further_media_bytes() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().canonicalize().unwrap().join("owned.wav");
    std::fs::write(&path, wav()).unwrap();
    let a = lease(&path);
    let id = a.id.clone();
    let store = Store::default();
    let a = store.insert(a).unwrap();
    assert!(store.live(&id));
    store.revoke(&id);
    assert!(!store.live(&id));
    let req = Request::builder()
        .header(header::RANGE, "bytes=0-43")
        .body(vec![])
        .unwrap();
    let res = response(&a, &req, "tauri://localhost", || {
        file_gate(&a, &[a.item.path.clone()])
    });
    assert_eq!(res.status(), 403);
    assert!(res.body().is_empty());
    let count = std::cell::Cell::new(0);
    let res = response(&a, &req, "tauri://localhost", || {
        count.set(count.get() + 1);
        if count.get() == 2 {
            std::fs::write(&path, b"owned changed").unwrap();
        }
        file_gate(&a, &[])
    });
    assert_eq!(res.status(), 403);
    assert!(res.body().is_empty());
    std::fs::write(&path, wav()).unwrap();
    let mut expired = lease(&path);
    expired.deadline = Instant::now() - Duration::from_secs(1);
    let id = expired.id.clone();
    store.insert(expired).unwrap();
    assert!(!store.live(&id));
    let current = lease(&path);
    let current_id = current.id.clone();
    store.insert(current).unwrap();
    store.revoke(&id);
    assert!(store.live(&current_id));
}
#[test]
fn capability_urls_range_limits_and_unsupported_formats_do_not_open_arbitrary_paths() {
    for path in [
        "/etc/passwd",
        "/%2e%2e/media",
        "/bad/media",
        "/12345678-1234-4123-8123-123456789abc/other",
        "/12345678-1234-4123-8123-123456789abc/media?path=/etc/passwd",
    ] {
        let r = Request::builder()
            .uri(format!("moomole-media://localhost{path}"))
            .body(vec![])
            .unwrap();
        assert!(token(&r).is_none());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let r = Request::builder()
        .uri(format!("moomole-media://localhost/{id}/media"))
        .body(vec![])
        .unwrap();
    assert_eq!(token(&r), Some(id.as_str()));
    for range_ in [
        "bytes=999999-",
        "bytes=5-2",
        "bytes=0-1,4-8",
        "bytes=-0",
        "bytes=x-2",
    ] {
        assert!(range(Some(range_), 100).is_err());
    }
    assert_eq!(range(Some("bytes=0-"), 10 * CHUNK), Ok((206, 0, CHUNK - 1)));
    assert!(range(None, CHUNK + 1).is_err());
    let t = tempfile::tempdir().unwrap();
    let path = t.path().canonicalize().unwrap().join("fake.mp4");
    std::fs::write(&path, b"https://example.invalid/not-a-movie").unwrap();
    assert!(prepare(&item(&path), &[]).is_err());
}
fn atom(kind: &[u8; 4], body: Vec<u8>) -> Vec<u8> {
    let mut a = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    a.extend(kind);
    a.extend(body);
    a
}
#[test]
fn reference_movies_encrypted_entries_and_out_of_bounds_boxes_are_refused() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().canonicalize().unwrap().join("owned.mp4");
    let data_ref = |flags: u32| {
        let mut b = vec![0; 4];
        b.extend(1u32.to_be_bytes());
        b.extend(atom(b"url ", flags.to_be_bytes().to_vec()));
        atom(b"dref", b)
    };
    let movie = |flags: u32| {
        let d = atom(b"dinf", data_ref(flags));
        let track = atom(b"trak", atom(b"mdia", atom(b"minf", d)));
        let mut v = atom(b"ftyp", b"isom\0\0\0\0isom".to_vec());
        v.extend(atom(b"moov", track));
        v.extend(atom(b"mdat", vec![0; 8]));
        v
    };
    std::fs::write(&path, movie(1)).unwrap();
    assert!(prepare(&item(&path), &[]).is_ok());
    std::fs::write(&path, movie(0)).unwrap();
    assert!(prepare(&item(&path), &[]).is_err());
    let mut invalid = movie(1);
    invalid[..4].copy_from_slice(&u32::MAX.to_be_bytes());
    std::fs::write(&path, invalid).unwrap();
    assert!(prepare(&item(&path), &[]).is_err());
    let mut linked = atom(b"moov", atom(b"rmra", vec![]));
    linked.extend(atom(b"mdat", vec![]));
    std::fs::write(&path, linked).unwrap();
    assert!(prepare(&item(&path), &[]).is_err());
    let mut stsd = vec![0; 4];
    stsd.extend(1u32.to_be_bytes());
    stsd.extend(atom(b"encv", vec![]));
    let mut count = 0;
    let mut refs = 0;
    assert!(containers::boxes(&atom(b"stsd", stsd), 0, &mut count, &mut refs).is_err());
}
