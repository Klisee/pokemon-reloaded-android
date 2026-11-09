use super::*;

fn lines(items: &[&[u8]]) -> Vec<Vec<u8>> {
    items.iter().map(|line| line.to_vec()).collect()
}

fn wcfg_payload(mut value: impl FnMut(usize, &WcfgFieldSpec) -> Vec<u8>) -> Vec<u8> {
    let parts: Vec<Vec<u8>> = WCFG_FIELD_SPECS.iter().enumerate().map(|(index, spec)| value(index, spec)).collect();
    parts.join(&FIELD_SEPARATOR)
}

fn wcfg_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = b"WCFG\x1f".to_vec();
    frame.extend_from_slice(payload);
    frame
}

fn replace_field(payload: &[u8], index: usize, value: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = payload.split(|&byte| byte == FIELD_SEPARATOR).collect();
    parts[index] = value;
    parts.join(&FIELD_SEPARATOR)
}

fn team_frame(meta: &[&[u8]; 5], body: &[u8]) -> Vec<u8> {
    let mut frame = b"TEAM\x1f".to_vec();
    for value in meta {
        frame.extend_from_slice(value);
        frame.push(RECORD_SEPARATOR);
    }
    frame.extend_from_slice(body);
    frame
}

#[test]
fn legacy_handshake_is_accepted() {
    let parsed = parse_world_handshake(&lines(&[b"WJOIN Klisee", b"WVER 6", b"WBUILD 20261103"])).unwrap();
    assert_eq!(parsed.name, b"Klisee");
    assert_eq!(parsed.profile, ProtocolProfile::Legacy06);
    assert_eq!(parsed.version, WVER_LEGACY);
    assert_eq!(parsed.build, WBUILD_LEGACY);
    assert!(parsed.trainer_id.is_none());
    assert!(parsed.sprite.is_none());
}

#[test]
fn z063_handshake_is_accepted_with_identity() {
    let parsed = parse_world_handshake(&lines(&[
        b"WJOIN Klisee\r\n",
        b"WTID 00123\n",
        b"WTSPR 456\r\n",
        b"WVER 8\n",
        b"WBUILD 20261109\r\n",
    ]))
    .unwrap();
    assert_eq!(parsed.profile, ProtocolProfile::Zibelan063);
    let trainer = parsed.trainer_id.as_ref().unwrap();
    assert_eq!(trainer.digits(), b"00123");
    assert_eq!(trainer.value(), 123);
    assert_eq!(parsed.sprite, Some(456));
}

#[test]
fn z063_handshake_sprite_is_optional() {
    let parsed = parse_world_handshake(&lines(&[b"WJOIN Klisee", b"WTID 77", b"WVER 8", b"WBUILD 20261109"])).unwrap();
    assert_eq!(parsed.sprite, None);
    assert_eq!(parsed.trainer_id.unwrap().value(), 77);
}

#[test]
fn z063_requires_a_trainer_id() {
    assert_eq!(
        parse_world_handshake(&lines(&[b"WJOIN Klisee", b"WVER 8", b"WBUILD 20261109"])),
        Err(HandshakeError::MissingTrainerId)
    );
}

#[test]
fn handshake_enforces_command_order() {
    let cases: &[&[&[u8]]] = &[
        &[b"WTID 1", b"WJOIN Klisee", b"WVER 8", b"WBUILD 20261109"],
        &[b"WJOIN Klisee", b"WTSPR 1", b"WVER 8", b"WBUILD 20261109"],
        &[b"WJOIN Klisee", b"WTID 1", b"WVER 8", b"WTSPR 1", b"WBUILD 20261109"],
        &[b"WJOIN Klisee", b"WBUILD 20261109", b"WVER 8"],
        &[b"WJOIN Klisee", b"WTID 1", b"WVER 8", b"WVER 8", b"WBUILD 20261109"],
        &[b"WJOIN Klisee", b"WTID 1", b"WVER 8", b"WBUILD 20261109", b"WSYNC"],
    ];
    for case in cases {
        assert!(parse_world_handshake(&lines(case)).is_err(), "{case:?}");
    }
}

#[test]
fn version_and_build_must_be_the_exact_pair() {
    let cases: &[(&[u8], &[u8])] = &[
        (b"8", b"20261103"),
        (b"6", b"20261109"),
        (b"7", b"20261109"),
        (b"8", b"20261108"),
        (b"08", b"20261109"),
        (b"8", b"020261109"),
        (b"+8", b"20261109"),
        (b"8", b"+20261109"),
        (b"", b"20261109"),
        (b"8", b""),
    ];
    for (version, build) in cases {
        let mut case = vec![b"WJOIN Klisee".to_vec(), b"WTID 1".to_vec()];
        case.push([b"WVER ", *version].concat());
        case.push([b"WBUILD ", *build].concat());
        assert!(parse_world_handshake(&case).is_err(), "{version:?} {build:?}");
    }
    assert_eq!(protocol_profile(WVER_LEGACY, WBUILD_LEGACY), Ok(ProtocolProfile::Legacy06));
    assert_eq!(protocol_profile(WVER_063, WBUILD_063), Ok(ProtocolProfile::Zibelan063));
    assert_eq!(protocol_profile(WVER_063, WBUILD_LEGACY), Err(HandshakeError::VersionBuildMismatch));
}

#[test]
fn trainer_id_bounds_are_enforced() {
    let max = validate_trainer_id(b"4294967295").unwrap();
    assert_eq!(max.value(), u64::from(u32::MAX));
    let padded = validate_trainer_id(b"00000").unwrap();
    assert_eq!(padded.digits(), b"00000");
    assert_eq!(padded.value(), 0);

    for bad in [b"".as_slice(), b"12a", b"-1", b"+1", b"1 2", b"................................................"] {
        assert!(validate_trainer_id(bad).is_err(), "{bad:?}");
    }
    assert_eq!(validate_trainer_id(b"12345678901"), Err(HandshakeError::TrainerIdTooLong));
    assert_eq!(validate_trainer_id(b"4294967296"), Err(HandshakeError::TrainerIdOverflow));
}

#[test]
fn sprite_bounds_include_the_unset_sentinel() {
    for good in [-1, 0, 1, 3_856] {
        assert_eq!(validate_sprite(good), Ok(good as i32), "{good}");
    }
    for bad in [-2, 3_857, i64::MAX, i64::MIN] {
        assert_eq!(validate_sprite(bad), Err(HandshakeError::SpriteOutOfRange), "{bad}");
    }
    assert_eq!(
        parse_world_handshake(&lines(&[b"WJOIN Klisee", b"WTID 1", b"WTSPR 3857", b"WVER 8", b"WBUILD 20261109",])),
        Err(HandshakeError::SpriteOutOfRange)
    );
    assert_eq!(
        parse_world_handshake(&lines(&[b"WJOIN Klisee", b"WTSPR -1", b"WVER 8", b"WBUILD 20261109",])),
        Err(HandshakeError::Order)
    );
}

#[test]
fn join_name_is_bounded_and_control_free() {
    let sixteen = b"abcdefghijklmnop";
    assert!(
        parse_world_handshake(&lines(&[
            [b"WJOIN ".as_slice(), sixteen].concat().as_slice(),
            b"WVER 6",
            b"WBUILD 20261103",
        ]))
        .is_ok()
    );

    let seventeen = b"abcdefghijklmnopq";
    for name in [b"".as_slice(), seventeen] {
        let case = lines(&[[b"WJOIN ".as_slice(), name].concat().as_slice(), b"WVER 6", b"WBUILD 20261103"]);
        assert!(parse_world_handshake(&case).is_err(), "{name:?}");
    }

    let control = lines(&[b"WJOIN a\x1fb", b"WVER 6", b"WBUILD 20261103"]);
    assert_eq!(parse_world_handshake(&control), Err(HandshakeError::NameControl));
}

#[test]
fn wcfg_field_table_matches_the_gml_order() {
    assert_eq!(WCFG_FIELD_COUNT, 37);
    assert_eq!(WCFG_MAX_FIELD_COUNT, 40);
    assert_eq!(WCFG_NUMERIC_FIELD_COUNT, 32);
    assert_eq!(WCFG_TEXT_FIELD_COUNT, 5);
    let names = [
        "NUM_POKEMON1",
        "NUM_POKEMON2",
        "DIFICULTAD1",
        "DIFICULTAD2",
        "CAMBIOS1",
        "CAMBIOS2",
        "NUM_OBJ1",
        "NUM_OBJ2",
        "OBJ_ASG1",
        "OBJ_ASG2",
        "CONT_POKE1",
        "CONT_POKE2",
        "VEL_ATAQUE1",
        "VEL_ATAQUE2",
        "REST_PP1",
        "REST_PP2",
        "SHOW_TYPE",
        "TIME_BATTLE",
        "BTTBACK_INDEX",
        "MUSIC_INDEX",
        "SUBITA",
        "BATTLE_MODE",
        "DAY",
        "shownumber",
        "showbattleevents",
        "showpanelcombate",
        "shownumberPS",
        "modeBattleBack",
        "WorldClima",
        "clima",
        "Hora",
        "Minuto",
        "REGLAS_J1",
        "REGLAS_J2",
        "OPTIONS_BT",
        "MUSIC_NAME",
        "txt_clima",
    ];
    for (index, name) in names.iter().enumerate() {
        let spec = wcfg_field_spec(index).unwrap();
        assert_eq!(spec.name, *name);
        assert_eq!(spec.kind == WcfgKind::Text, index >= WCFG_NUMERIC_FIELD_COUNT);
    }
    assert!(wcfg_field_spec(WCFG_FIELD_COUNT).is_none());
}

#[test]
fn wcfg_accepts_all_minimum_and_maximum_field_values() {
    let minimum = wcfg_payload(|_, spec| match spec.kind {
        WcfgKind::Number => spec.min.to_string().into_bytes(),
        WcfgKind::Text => Vec::new(),
    });
    let parsed = parse_wcfg_frame(&wcfg_frame(&minimum)).unwrap();
    for (index, spec) in WCFG_FIELD_SPECS.iter().enumerate() {
        match spec.kind {
            WcfgKind::Number => assert_eq!(parsed.number(index), Some(spec.min)),
            WcfgKind::Text => assert_eq!(parsed.text(index), Some(&b""[..])),
        }
    }

    let maximum = wcfg_payload(|_, spec| match spec.kind {
        WcfgKind::Number => spec.max.to_string().into_bytes(),
        WcfgKind::Text => b"text".to_vec(),
    });
    let parsed = parse_wcfg_frame(&wcfg_frame(&maximum)).unwrap();
    for (index, spec) in WCFG_FIELD_SPECS.iter().enumerate() {
        match spec.kind {
            WcfgKind::Number => assert_eq!(parsed.number(index), Some(spec.max)),
            WcfgKind::Text => assert_eq!(parsed.text(index), Some(&b"text"[..])),
        }
    }
}

#[test]
fn wcfg_rejects_values_outside_each_field_range() {
    let base = wcfg_payload(|_, spec| match spec.kind {
        WcfgKind::Number => spec.min.to_string().into_bytes(),
        WcfgKind::Text => b"ok".to_vec(),
    });
    for (index, spec) in WCFG_FIELD_SPECS.iter().enumerate() {
        if spec.kind != WcfgKind::Number {
            continue;
        }
        if spec.min > 0 {
            let wire = replace_field(&base, index, &(spec.min - 1).to_string().into_bytes());
            assert!(parse_wcfg_payload(&wire).is_err(), "field {index} accepted {}", spec.min - 1);
        } else {
            let wire = replace_field(&base, index, b"-1");
            assert!(parse_wcfg_payload(&wire).is_err(), "field {index} accepted -1");
        }
        let wire = replace_field(&base, index, &(spec.max + 1).to_string().into_bytes());
        assert!(parse_wcfg_payload(&wire).is_err(), "field {index} accepted {}", spec.max + 1);
    }
}

#[test]
fn wcfg_accepts_the_gml_default_configuration() {
    let defaults =
        [6, 6, 1, 1, 18, 18, 6, 6, 15, 15, 1, 1, 0, 0, 0, 0, 1, 0, 42, 161, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 8, 30];
    let payload = wcfg_payload(|index, spec| match spec.kind {
        WcfgKind::Number => defaults[index].to_string().into_bytes(),
        WcfgKind::Text => b"6#default".to_vec(),
    });
    let parsed = parse_wcfg_frame(&wcfg_frame(&payload)).unwrap();
    assert_eq!(parsed.number(0), Some(6));
    assert_eq!(parsed.number(18), Some(42));
    assert_eq!(parsed.number(30), Some(8));
    assert_eq!(parsed.number(31), Some(30));
    assert_eq!(parsed.text(32), Some(&b"6#default"[..]));
}

#[test]
fn wcfg_field_count_accepts_the_37_field_wire_and_bounded_extras() {
    let payload = wcfg_payload(|_, spec| match spec.kind {
        WcfgKind::Number => spec.min.to_string().into_bytes(),
        WcfgKind::Text => b"x".to_vec(),
    });
    assert!(parse_wcfg_payload(&payload).is_ok());

    let mut with_one_extra = payload.clone();
    with_one_extra.push(FIELD_SEPARATOR);
    with_one_extra.extend_from_slice(b"extra");
    assert!(parse_wcfg_payload(&with_one_extra).is_ok());

    let mut with_three_extras = payload.clone();
    for extra in [b"a", b"b", b"c"] {
        with_three_extras.push(FIELD_SEPARATOR);
        with_three_extras.extend_from_slice(extra);
    }
    let parsed = parse_wcfg_payload(&with_three_extras).unwrap();
    assert_eq!(parsed.extras(), &[b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);

    let mut with_four_extras = with_three_extras;
    with_four_extras.push(FIELD_SEPARATOR);
    with_four_extras.extend_from_slice(b"d");
    assert_eq!(parse_wcfg_payload(&with_four_extras), Err(WcfgError::FieldCount { actual: WCFG_MAX_FIELD_COUNT + 1 }));

    let short = replace_field(&payload, 36, b"");
    let short_parts: Vec<&[u8]> = short.split(|&byte| byte == FIELD_SEPARATOR).collect();
    let short_wire = short_parts[..WCFG_FIELD_COUNT - 1].join(&FIELD_SEPARATOR);
    assert_eq!(parse_wcfg_payload(&short_wire), Err(WcfgError::FieldCount { actual: WCFG_FIELD_COUNT - 1 }));
}

#[test]
fn wcfg_accepts_only_the_emitted_background_ranges() {
    for good in [1, 253, 1_000, 1_063, 2_000, 2_004] {
        assert!(is_valid_battle_background(good), "{good}");
    }
    for bad in [0, 254, 999, 1_064, 1_999, 2_005] {
        assert!(!is_valid_battle_background(bad), "{bad}");
    }
    let base = wcfg_payload(|index, spec| match spec.kind {
        WcfgKind::Number => {
            if index == 18 {
                b"42".to_vec()
            } else {
                spec.min.to_string().into_bytes()
            }
        },
        WcfgKind::Text => b"x".to_vec(),
    });
    for bad in [0, 254, 999, 1_064, 1_999, 2_005] {
        let wire = replace_field(&base, 18, &bad.to_string().into_bytes());
        assert!(parse_wcfg_payload(&wire).is_err(), "background {bad}");
    }
}

#[test]
fn wcfg_rejects_empty_noncanonical_and_control_values() {
    let base = wcfg_payload(|_, spec| match spec.kind {
        WcfgKind::Number => spec.min.to_string().into_bytes(),
        WcfgKind::Text => b"x".to_vec(),
    });
    for bad in [b"".as_slice(), b"1.0", b"01", b"+1", b" 1", b"1 ", b"0x10"] {
        let wire = replace_field(&base, 0, bad);
        assert!(parse_wcfg_payload(&wire).is_err(), "{bad:?}");
    }
    for bad in [b"a\nb".as_slice(), b"a\rb", b"a\0b", b"a\x7fb"] {
        let wire = replace_field(&base, 32, bad);
        assert!(parse_wcfg_payload(&wire).is_err(), "{bad:?}");
    }
    let oversized = vec![b'a'; MAX_WCFG_TEXT_BYTES + 1];
    let wire = replace_field(&base, 36, &oversized);
    assert_eq!(parse_wcfg_payload(&wire), Err(WcfgError::TextTooLarge { index: 36 }));
}

#[test]
fn wcfg_preserves_non_utf8_text_for_compatibility() {
    let base = wcfg_payload(|_, spec| match spec.kind {
        WcfgKind::Number => spec.min.to_string().into_bytes(),
        WcfgKind::Text => b"x".to_vec(),
    });
    let raw = [0x80, 0xff, 0x41, 0xc3, 0xa9];
    let wire = replace_field(&base, 35, &raw);
    let parsed = parse_wcfg_payload(&wire).unwrap();
    assert_eq!(parsed.text(35), Some(&raw[..]));
}

#[test]
fn wcfg_frame_requires_tag_separator_and_bounded_size() {
    let payload = wcfg_payload(|_, spec| match spec.kind {
        WcfgKind::Number => spec.min.to_string().into_bytes(),
        WcfgKind::Text => b"x".to_vec(),
    });
    assert_eq!(parse_wcfg_frame(&payload), Err(WcfgError::MissingTag));
    assert_eq!(parse_wcfg_frame(b"WCFG"), Err(WcfgError::MissingTagSeparator));
    assert_eq!(parse_wcfg_frame(b"WCFG "), Err(WcfgError::MissingTagSeparator));
    let mut huge = b"WCFG\x1f".to_vec();
    huge.extend(std::iter::repeat_n(b'a', MAX_WCFG_BYTES + 1));
    assert_eq!(parse_wcfg_frame(&huge), Err(WcfgError::FrameTooLarge));
}

#[test]
fn team_meta_accepts_the_gml_values() {
    let frame = team_frame(&[b"42", b"1000", b"0", b"47", b"5"], b"team-body\x1e-with-rs");
    let parsed = parse_team_frame(&frame).unwrap();
    assert_eq!(parsed.meta, TeamMeta { seed: 42, fondo: 1_000, modo: 0, sala: 47, delay: 5 });
    assert_eq!(parsed.body, b"team-body\x1e-with-rs");
}

#[test]
fn team_meta_boundaries_are_enforced() {
    let good = [
        TeamMeta { seed: 1, fondo: 1, modo: 0, sala: 0, delay: 2 },
        TeamMeta { seed: 1_000_000_000, fondo: 2_004, modo: 2, sala: 1_000, delay: 10 },
        TeamMeta { seed: 1, fondo: 253, modo: 1, sala: 47, delay: 2 },
        TeamMeta { seed: 1, fondo: 1_000, modo: 2, sala: 191, delay: 10 },
        TeamMeta { seed: 1, fondo: 2_000, modo: 0, sala: 709, delay: 5 },
    ];
    for meta in good {
        assert_eq!(validate_team_meta(meta), Ok(()), "{meta:?}");
    }

    let bad = [
        TeamMeta { seed: 0, ..good[0] },
        TeamMeta { seed: 1_000_000_001, ..good[0] },
        TeamMeta { fondo: 0, ..good[0] },
        TeamMeta { fondo: 254, ..good[0] },
        TeamMeta { fondo: 999, ..good[0] },
        TeamMeta { fondo: 1_064, ..good[0] },
        TeamMeta { fondo: 1_999, ..good[0] },
        TeamMeta { fondo: 2_005, ..good[0] },
        TeamMeta { modo: 3, ..good[0] },
        TeamMeta { sala: 1_001, ..good[0] },
        TeamMeta { delay: 1, ..good[0] },
        TeamMeta { delay: 11, ..good[0] },
    ];
    for meta in bad {
        assert!(validate_team_meta(meta).is_err(), "{meta:?}");
    }
}

#[test]
fn team_requires_five_metadata_fields() {
    for count in 0..TEAM_META_FIELD_COUNT {
        let mut frame = b"TEAM\x1f".to_vec();
        for index in 0..count {
            frame.extend_from_slice(if index == 0 { b"1".as_slice() } else { b"1" });
            frame.push(RECORD_SEPARATOR);
        }
        frame.extend_from_slice(b"body");
        assert_eq!(parse_team_frame(&frame), Err(TeamError::MetadataCount { actual: count }));
    }
}

#[test]
fn team_rejects_empty_and_noncanonical_metadata() {
    let cases: [&[u8]; 6] = [b"", b"01", b"+1", b"1.0", b" 1", b"1 "];
    for bad in cases {
        let frame = team_frame(&[bad, b"1", b"0", b"0", b"2"], b"body");
        assert!(parse_team_frame(&frame).is_err(), "{bad:?}");
    }
}

#[test]
fn team_frame_requires_tag_separator_and_bounded_body() {
    assert_eq!(parse_team_frame(b"TEAM"), Err(TeamError::MissingTagSeparator));
    assert_eq!(parse_team_frame(b"WCFG\x1f"), Err(TeamError::MissingTag));
    let mut frame = team_frame(&[b"1", b"1", b"0", b"0", b"2"], b"");
    frame.extend(std::iter::repeat_n(b'a', MAX_TEAM_BYTES + 1));
    assert_eq!(parse_team_frame(&frame), Err(TeamError::FrameTooLarge));
}

#[test]
fn checksum_text_matches_the_gml_rolling_formula() {
    assert_eq!(checksum_text(b""), 0);
    assert_eq!(checksum_text(b"A"), 65);
    assert_eq!(checksum_text(b"AB"), 65 * 31 + 66);
    assert_eq!(checksum_text(b"ABC"), 64_578);
    let long = vec![b'a'; 10_000];
    let mut expected = 0u64;
    for &byte in &long {
        expected = (expected * 31 + u64::from(byte)) % CHECKSUM_MODULUS as u64;
    }
    assert_eq!(u64::from(checksum_text(&long)), expected);
    assert!(checksum_text(&long) <= MAX_CHECKSUM as u32);
}

#[test]
fn checksum_frame_and_mask_validation() {
    assert_eq!(validate_checksum(0), Ok(0));
    assert_eq!(validate_checksum(MAX_CHECKSUM), Ok(MAX_CHECKSUM as u32));
    assert_eq!(validate_checksum(-1), Err(GuardError::ChecksumOutOfRange));
    assert_eq!(validate_checksum(MAX_CHECKSUM + 1), Err(GuardError::ChecksumOutOfRange));

    assert_eq!(validate_frame(0), Ok(0));
    assert_eq!(validate_frame(i32::MAX), Ok(i32::MAX as u32));
    assert_eq!(validate_frame(-1), Err(GuardError::NegativeFrame));

    assert_eq!(validate_input_mask(0), Ok(0));
    assert_eq!(validate_input_mask(MAX_INPUT_MASK as i32), Ok(MAX_INPUT_MASK));
    assert_eq!(validate_input_mask(-1), Err(GuardError::MaskOutOfRange));
    assert_eq!(validate_input_mask(MAX_INPUT_MASK as i32 + 1), Err(GuardError::MaskOutOfRange));
}

#[test]
fn sum_and_bye_frames_are_parsed_with_their_real_separators() {
    assert_eq!(parse_sum_frame(b"SUM\x1f123\x1e456"), Ok((123, 456)));
    assert_eq!(parse_sum_frame(b"SUM\x1f123 456"), Ok((123, 456)));
    assert_eq!(parse_bye_frame(b"BYE\x1f42 300"), Ok((42, 300)));
    assert_eq!(parse_bye_frame(b"BYE\x1f-1 0"), Ok((-1, 0)));

    assert_eq!(parse_sum_frame(b"SUM"), Err(BattleFrameError::MissingTagSeparator));
    assert_eq!(parse_sum_frame(b"WCFG\x1f123\x1e456"), Err(BattleFrameError::MissingTag));
    assert_eq!(parse_sum_frame(b"SUM\x1f123"), Err(BattleFrameError::Malformed));
    assert_eq!(parse_sum_frame(b"SUM\x1f-1\x1e456"), Err(BattleFrameError::NegativeFrame));
    assert_eq!(parse_sum_frame(b"SUM\x1f1\x1e1000003"), Err(BattleFrameError::ChecksumOutOfRange));
    assert_eq!(parse_bye_frame(b"BYE"), Err(BattleFrameError::MissingTagSeparator));
    assert_eq!(parse_bye_frame(b"SUM\x1f1 2"), Err(BattleFrameError::MissingTag));
    assert_eq!(parse_bye_frame(b"BYE\x1f1"), Err(BattleFrameError::Malformed));
    assert_eq!(parse_bye_frame(b"BYE\x1f1 -2"), Err(BattleFrameError::NegativeFrame));
    assert_eq!(parse_bye_frame(b"BYE\x1f2147483648 1"), Err(BattleFrameError::Malformed));
}

#[test]
fn guard_inputs_are_windowed_and_replay_safe() {
    let mut guard = BattleGuard::new(7);
    guard.advance_local_frame(300);
    assert_eq!(guard.receive_input(7, 300, 0x05), Ok(InputDecision::Accepted { frame: 300, mask: 0x05 }));
    assert_eq!(guard.receive_input(7, 300, 0x05), Ok(InputDecision::IgnoreDuplicate));
    assert_eq!(guard.receive_input(7, 300, 0x06), Ok(InputDecision::RejectConflictingReplay));
    assert_eq!(guard.receive_input(8, 300, 0x07), Ok(InputDecision::IgnoreStaleEpoch));
    assert_eq!(guard.receive_input(7, 43, 0x00), Ok(InputDecision::IgnoreStaleFrame));
    assert_eq!(guard.receive_input(7, 557, 0x00), Ok(InputDecision::IgnoreFutureFrame));
    assert_eq!(
        guard.receive_input(7, 44, MAX_INPUT_MASK as i32),
        Ok(InputDecision::Accepted { frame: 44, mask: MAX_INPUT_MASK })
    );
    assert_eq!(guard.receive_input(7, 556, 0x00), Ok(InputDecision::Accepted { frame: 556, mask: 0 }));
    assert_eq!(guard.receive_input(7, -1, 0), Err(GuardError::NegativeFrame));
    assert_eq!(guard.receive_input(7, 300, -1), Err(GuardError::MaskOutOfRange));
}

#[test]
fn guard_checksums_compare_only_the_same_frame_and_return_once() {
    let mut guard = BattleGuard::new(9);
    guard.advance_local_frame(300);
    assert_eq!(guard.note_local_checksum(300, 10), Ok(None));
    assert_eq!(guard.receive_checksum(9, 300, 10), Ok(ChecksumDecision::Accepted));
    assert_eq!(guard.receive_checksum(9, 300, 10), Ok(ChecksumDecision::IgnoreDuplicate));
    assert_eq!(guard.receive_checksum(9, 300, 11), Ok(ChecksumDecision::RejectConflictingReplay));
    assert_eq!(guard.receive_checksum(8, 300, 12), Ok(ChecksumDecision::IgnoreStaleEpoch));

    let mut guard = BattleGuard::new(9);
    guard.advance_local_frame(300);
    assert_eq!(guard.receive_checksum(9, 300, 20), Ok(ChecksumDecision::Accepted));
    assert_eq!(guard.note_local_checksum(300, 10), Ok(Some(Desync { frame: 300, local: 10, peer: 20 })));
    assert!(guard.desync());
    assert_eq!(guard.note_local_checksum(300, 10), Ok(None));
}

#[test]
fn guard_desync_signal_fires_once_per_frame() {
    let mut guard = BattleGuard::new(1);
    guard.advance_local_frame(300);
    guard.note_local_checksum(300, 10).unwrap();
    assert_eq!(
        guard.receive_checksum(1, 300, 99),
        Ok(ChecksumDecision::Desync(Desync { frame: 300, local: 10, peer: 99 }))
    );
    assert_eq!(guard.receive_checksum(1, 300, 99), Ok(ChecksumDecision::IgnoreDuplicate));
    assert_eq!(guard.receive_checksum(1, 300, 98), Ok(ChecksumDecision::RejectConflictingReplay));
    assert!(guard.desync());
}

#[test]
fn guard_ignores_stale_and_future_checksums() {
    let mut guard = BattleGuard::new(1);
    guard.advance_local_frame(3_900);
    assert_eq!(guard.receive_checksum(1, 3_659, 1), Ok(ChecksumDecision::IgnoreStaleFrame));
    assert_eq!(guard.receive_checksum(1, 3_660, 1), Ok(ChecksumDecision::Accepted));
    assert_eq!(guard.receive_checksum(1, 4_140, 1), Ok(ChecksumDecision::Accepted));
    assert_eq!(guard.receive_checksum(1, 4_141, 1), Ok(ChecksumDecision::IgnoreFutureFrame));
    assert_eq!(guard.receive_checksum(1, -1, 1), Err(GuardError::NegativeFrame));
    assert_eq!(guard.receive_checksum(1, 3_900, -1), Err(GuardError::ChecksumOutOfRange));
    assert_eq!(guard.receive_checksum(1, 3_900, MAX_CHECKSUM + 1), Err(GuardError::ChecksumOutOfRange));
}

#[test]
fn guard_replayed_old_checksum_cannot_force_a_desync() {
    let mut guard = BattleGuard::new(4);
    guard.advance_local_frame(600);
    guard.note_local_checksum(300, 7).unwrap();
    assert_eq!(guard.receive_checksum(4, 300, 8), Ok(ChecksumDecision::IgnoreStaleFrame));
    assert!(!guard.desync());
}

#[test]
fn guard_rejects_local_checksum_contradictions() {
    let mut guard = BattleGuard::new(4);
    guard.note_local_checksum(300, 7).unwrap();
    assert_eq!(guard.note_local_checksum(300, 8), Err(GuardError::LocalContradiction));
}

#[test]
fn guard_bye_is_epoch_bound_and_replay_safe() {
    let mut guard = BattleGuard::new(7);
    guard.advance_local_frame(100);
    assert_eq!(guard.receive_bye(8, 50), Ok(ByeDecision::IgnoreStaleEpoch));
    assert_eq!(guard.receive_bye(7, 200), Ok(ByeDecision::Accepted { frame: 200 }));
    assert_eq!(guard.bye_frame(), Some(200));
    assert_eq!(guard.receive_bye(7, 200), Ok(ByeDecision::IgnoreDuplicate));
    assert_eq!(guard.receive_bye(7, 201), Ok(ByeDecision::RejectConflictingReplay));
    assert_eq!(guard.receive_bye(7, 2_021), Ok(ByeDecision::IgnoreFutureFrame));
    assert!(!guard.should_leave());
    guard.advance_local_frame(201);
    assert!(guard.should_leave());
}

#[test]
fn guard_advance_prunes_and_keeps_latest_frame() {
    let mut guard = BattleGuard::new(2);
    guard.advance_local_frame(10);
    guard.receive_input(2, 10, 1).unwrap();
    guard.advance_local_frame(9);
    assert_eq!(guard.local_frame(), 10);
    guard.advance_local_frame(1_000);
    assert_eq!(guard.receive_input(2, 10, 1), Ok(InputDecision::IgnoreStaleFrame));
}
