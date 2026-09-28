use super::*;

#[test]
fn cache_records_bind_every_header_byte_pixels_and_full_render_identity() {
    let spec = ImageSpec::new(8, 8, ScaleMode::Contain).unwrap();
    let key = ImageKey::file("ONE.PNG", 100, 5, spec).unwrap();
    let image = PreparedImage {
        key: key.clone(),
        source: Size::new(80, 80).unwrap(),
        state: CacheState::Prepared,
    };
    let pixels = [0x55; 16];
    let header = image.encode(&pixels);
    let parsed = PreparedImage::decode(&header, &key, &pixels).unwrap();
    assert_eq!(parsed.key.spec(), spec);
    assert_eq!(parsed.state, CacheState::Hit);
    for index in 0..CACHE_HEADER_BYTES {
        let mut damaged = header;
        damaged[index] ^= 1;
        assert!(
            PreparedImage::decode(&damaged, &key, &pixels).is_none(),
            "header byte {index}"
        );
    }
    for index in [0, 8, 410, 416, 428] {
        let mut damaged = header;
        damaged[index] ^= 1;
        let checksum = crc32fast::hash(&damaged[..424]);
        damaged[424..428].copy_from_slice(&checksum.to_le_bytes());
        assert!(
            PreparedImage::decode(&damaged, &key, &pixels).is_none(),
            "valid CRC must not conceal a changed format/key/length: {index}"
        );
    }
    let mut zero_width = header;
    zero_width[412..414].fill(0);
    let checksum = crc32fast::hash(&zero_width[..424]);
    zero_width[424..428].copy_from_slice(&checksum.to_le_bytes());
    assert!(PreparedImage::decode(&zero_width, &key, &pixels).is_none());
    for index in 0..pixels.len() {
        let mut damaged = pixels;
        damaged[index] ^= 1;
        assert!(
            PreparedImage::decode(&header, &key, &damaged).is_none(),
            "pixel byte {index}"
        );
    }
    for other in [
        ImageKey::file("TWO.PNG", 100, 5, spec).unwrap(),
        ImageKey::file("ONE.PNG", 101, 5, spec).unwrap(),
        ImageKey::file("ONE.PNG", 100, 6, spec).unwrap(),
        ImageKey::file(
            "ONE.PNG",
            100,
            5,
            ImageSpec::new(8, 8, ScaleMode::Cover).unwrap(),
        )
        .unwrap(),
        ImageKey::file(
            "ONE.PNG",
            100,
            5,
            ImageSpec::new(16, 4, ScaleMode::Contain).unwrap(),
        )
        .unwrap(),
    ] {
        assert!(PreparedImage::decode(&header, &other, &pixels).is_none());
    }
    assert!(PreparedImage::decode(&header, &key, &pixels[..15]).is_none());
}

#[test]
fn packed_render_specs_cannot_exceed_the_frame_or_require_fractional_strides() {
    for (width, height) in [
        (0, 1),
        (7, 1),
        (481, 1),
        (8, 0),
        (8, 801),
        (usize::MAX, usize::MAX),
    ] {
        assert!(ImageSpec::new(width, height, ScaleMode::Contain).is_none());
    }
    assert_eq!(
        ImageSpec::new(480, 800, ScaleMode::Contain)
            .unwrap()
            .byte_len(),
        96_000
    );
}
