use super::*;
use ed25519_dalek::{Signer, SigningKey};
fn job() -> Job {
    Job::new(TaskKind::AppControl)
}
fn signed(bytes: &[u8]) -> (String, Release) {
    let k = SigningKey::from_bytes(&[31; 32]);
    (
        STANDARD.encode(k.verifying_key().to_bytes()),
        Release {
            id: uuid::Uuid::new_v4().to_string(),
            version: "2.0".into(),
            build: "2".into(),
            url: "https://updates.example.org/owned.zip".into(),
            kind: "package",
            bytes: Some(bytes.len() as u64),
            requires_review: false,
            signature: Some(STANDARD.encode(k.sign(bytes).to_bytes())),
            downloadable: true,
        },
    )
}
#[test]
fn strict_signature_verification_matches_rfc8032_and_refuses_tampering_and_weak_keys() {
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
    // RFC 8032 section 7.1, TEST 2, independently published one-byte message.
    let k = STANDARD.encode(hex(
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
    ));
    let (_, mut r) = signed(b"r");
    r.signature=Some(STANDARD.encode(hex("92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00")));
    assert!(verify(b"r", &r, &k).is_ok());
    assert!(verify(b"s", &r, &k).is_err());
    assert!(verify(b"rr", &r, &k).is_err());
    assert!(key(&STANDARD.encode([0; 32])).is_err());
    r.bytes = None;
    assert!(verify(b"r", &r, &k).is_err());
    assert!(signature("short").is_err());
}
#[test]
fn signed_delivery_preserves_quarantine_and_originals_without_partial_file_accumulation() {
    let temp = tempfile::Builder::new()
        .prefix("moomole-download-owned-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let directory = root.join("Deliveries");
    std::fs::create_dir(&directory).unwrap();
    let keep = directory.join("keep.txt");
    std::fs::write(&keep, b"unselected").unwrap();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "Owned.app/Contents/Info.plist",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"owned inert fixture, not installed or launched")
        .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let (key, release) = signed(&bytes);
    let mut input = tempfile::NamedTempFile::new().unwrap();
    input.write_all(&bytes).unwrap();
    crate::update_quarantine::mark_from(
        input.path(),
        &release.url,
        "https://updates.example.org/feed.xml",
    )
    .unwrap();
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let (path, hash) = save(
        input.as_file_mut(),
        &release,
        &key,
        directory.to_str().unwrap(),
        &settings,
        &job(),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(hash, format!("{:x}", Sha256::digest(&bytes)));
    crate::update_quarantine::present(Path::new(&path)).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read(input.path()).unwrap(), bytes);
    assert_eq!(std::fs::read(&keep).unwrap(), b"unselected");
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
    for mode in [
        "tampered",
        "cancelled",
        "protected",
        "revoked",
        "parentChanged",
    ] {
        let mut bad = input.reopen().unwrap();
        let mut r = release.clone();
        let mut s = settings.clone();
        let j = job();
        match mode {
            "tampered" => r.signature = signed(b"other").1.signature,
            "cancelled" => j.cancel(),
            "protected" => s.protected_paths.push(directory.to_str().unwrap().into()),
            _ => {}
        }
        let result = save(
            &mut bad,
            &r,
            &key,
            directory.to_str().unwrap(),
            &s,
            &j,
            || {
                if mode == "revoked" {
                    return Err("revoked before publication".into());
                }
                if mode == "parentChanged" {
                    std::fs::rename(&directory, root.join("original-directory")).unwrap();
                    std::fs::create_dir(&directory).unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_err(), "{mode}");
        if mode == "parentChanged" {
            assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
            std::fs::remove_dir(&directory).unwrap();
            std::fs::rename(root.join("original-directory"), &directory).unwrap();
        }
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2, "{mode}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let delivered = delivery(
        "7f786cf6-756c-49a7-b5a7-397fbb7cd15d",
        &release,
        "7205853c-c7fd-47a9-8e83-fcebe84336dd".into(),
        path.clone(),
        hash.clone(),
    );
    let proof = serde_json::json!({"delivery":delivered,"directory":directory,"candidate":release,"signedPackageSaved":true,"bytes":bytes.len(),"sha256":hash,"systemQuarantinePreserved":true,"fileMode":"0600","sourceAndUnselectedPreserved":true,"failurePartialsRemoved":true,"parentReplacementRefused":true,"fixture":root,"network":false,"appInstalledOrLaunched":false});
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write(
        "../.cache/app-download-native.json",
        serde_json::to_vec_pretty(&proof).unwrap(),
    )
    .unwrap();
}
#[test]
fn only_complete_signed_bounded_packages_are_download_candidates() {
    let (_, r) = signed(b"owned");
    let xml = format!(
        r#"<rss xmlns:s="http://www.andymatuschak.org/xml-namespaces/sparkle"><channel><item><s:version>2</s:version><enclosure url="{}" length="5" s:edSignature="{}"/></item></channel></rss>"#,
        r.url,
        r.signature.unwrap()
    );
    let parsed = feed::parse(xml.as_bytes(), "1", "27.0", "arm64", &job()).unwrap();
    assert!(parsed.0[0].downloadable);
    let duplicate=xml.replace("</channel>",&format!("<item><s:version>2</s:version><enclosure url=\"{}\" length=\"6\" s:edSignature=\"{}\"/></item></channel>",r.url,STANDARD.encode([7;64])));
    let parsed = feed::parse(duplicate.as_bytes(), "1", "27.0", "arm64", &job()).unwrap();
    assert!(!parsed.3);
    assert!(!parsed.0[0].downloadable);
    for changed in [
        xml.replace("length=\"5\"", "length=\"0\""),
        xml.replace("length=\"5\"", "length=\"268435457\""),
        xml.replace(".zip\"", ".pkg\""),
        xml.replace("edSignature=", "dsaSignature="),
    ] {
        let rows = feed::parse(changed.as_bytes(), "1", "27.0", "arm64", &job())
            .unwrap()
            .0;
        assert!(!rows[0].downloadable);
    }
}
#[test]
#[ignore = "explicit public Rectangle appcast and signed package download into an automatically removed system TempDir; no installation, mounting or launch"]
fn real_signed_package_https_download_and_save_without_installing() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let job = Arc::new(job());
    let feed_url = "https://rectangleapp.com/downloads/updates.xml";
    let body = runtime
        .block_on(network::fetch(feed_url, job.clone()))
        .unwrap();
    let (rows, _, _, _) = feed::parse(&body, "0", "27.2", "arm64", &job).unwrap();
    let r = rows
        .iter()
        .find(|r| r.downloadable)
        .expect("supported signed full package");
    // Public key in Rectangle's upstream Info.plist, independently inspected.
    // This test is a delivery proof, not authority over an installed Rectangle.
    let k = "lpt9M3PhocbZ3MZiLH+crEqRfU11kfoNzGxSqiEIdvM=";
    let temp = tempfile::Builder::new()
        .prefix("moomole-public-download-owned-")
        .tempdir()
        .unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut input = tempfile::NamedTempFile::new().unwrap();
    runtime
        .block_on(fetch(r, job.clone(), input.as_file_mut()))
        .unwrap();
    crate::update_quarantine::mark_from(input.path(), &r.url, feed_url).unwrap();
    let settings = db::Settings {
        acknowledged: true,
        ..Default::default()
    };
    let (path, hash) = save(
        input.as_file_mut(),
        r,
        k,
        root.to_str().unwrap(),
        &settings,
        &job,
        || Ok(()),
    )
    .unwrap();
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(verify(&saved, r, k).unwrap(), hash);
    crate::update_quarantine::present(Path::new(&path)).unwrap();
    std::fs::create_dir_all("../.cache").unwrap();
    std::fs::write("../.cache/app-download-public-native.json",serde_json::to_vec_pretty(&serde_json::json!({"publicFeed":feed_url,"candidate":r,"downloadedBytes":saved.len(),"sha256":hash,"signatureMatchesIndependentUpstreamKey":true,"systemQuarantinePreserved":true,"fixture":root,"appInstalledMountedOrLaunched":false,"appWindowVerified":false})).unwrap()).unwrap();
}
