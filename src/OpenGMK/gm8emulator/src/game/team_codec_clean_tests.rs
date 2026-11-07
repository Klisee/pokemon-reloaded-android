use super::*;

const TEST_CURRENT_VERSION: u32 = 20260802;
const TEST_DEFAULT_VERSION: u32 = 20240713;
const TEST_CUTOVER_VERSION: u32 = 20241217;

const V001_LF: &str = r#"A
99
749104
53

6857
115
39
32657
3127
0
678
354
323
857
354
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
6852
78
34
32657
3127
29
678
354
323
857
354
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
6852
78
34
32657
3127
58
678
354
323
857
354
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
6852
78
34
32657
3127
87
678
354
323
857
354
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
6852
78
34
32657
3127
116
678
354
323
857
354
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
6852
78
34
32657
3127
145
678
354
323
857
354
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
0
2876568
20260802
"#;

fn round_half_even(k: i64) -> i64 {
    let floor = k / 2;
    if k % 2 == 0 || floor % 2 == 0 {
        floor
    } else {
        floor + 1
    }
}

fn reference_checksum_bytes(secret: &[u8]) -> u64 {
    let n = secret.len();
    let mut acc: i64 = 0;
    for k in 0..=n {
        let b = if k == 0 {
            secret.first().copied().unwrap_or(0) as i64
        } else {
            secret[k - 1] as i64
        };
        let kk = k as i64;
        let term = match k % 8 {
            0 => 7 * b + kk + 9,
            1 => 3 * b + 2 * kk,
            2 => 5 * b + kk + (kk + 2) / 3,
            3 => 9 * b + kk + 53,
            4 => 15 * b + 3 * kk,
            5 => (3 * b + kk) ^ 2,
            6 => 7 * b + 37 * kk,
            _ => 9 * b + kk + round_half_even(kk),
        };
        acc = acc.wrapping_add(term);
    }
    acc.wrapping_mul(7).wrapping_sub(20053).wrapping_abs() as u64
}

fn reference_checksum_lines(lines: &[Vec<u8>]) -> u64 {
    let mut secret = Vec::new();
    for line in lines {
        secret.extend_from_slice(line);
    }
    reference_checksum_bytes(&secret)
}

fn codec_checksum(lines: &[Vec<u8>]) -> u64 {
    let refs: Vec<&[u8]> = lines.iter().map(|line| line.as_slice()).collect();
    checksum(&refs).expect("checksum must accept 167 lines")
}

fn decode_default(input: &[u8]) -> Result<DecodeResult, TeamError> {
    decode(input, &DecodeOptions::default())
}

fn decode_strict(input: &[u8]) -> Result<DecodeResult, TeamError> {
    decode(
        input,
        &DecodeOptions {
            strict_canonical: true,
            ..DecodeOptions::default()
        },
    )
}

fn assert_decode_category(input: &[u8], expected: ErrorCategory) {
    match decode_default(input) {
        Ok(_) => panic!("expected {expected:?}, but decode succeeded"),
        Err(error) => assert_eq!(error.category, expected, "unexpected error: {error:?}"),
    }
}

fn split_lines(input: &[u8]) -> Vec<&[u8]> {
    let mut lines: Vec<&[u8]> = input
        .split(|&byte| byte == b'\n')
        .map(|line| {
            if line.last() == Some(&b'\r') {
                &line[..line.len() - 1]
            } else {
                line
            }
        })
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn build_file(payload: &[Vec<u8>], version: Option<&[u8]>, eol: &[u8], final_eol: bool) -> Vec<u8> {
    assert_eq!(payload.len(), 167);
    let stored = reference_checksum_lines(payload).to_string().into_bytes();
    build_file_with_stored_checksum(payload, &stored, version, eol, final_eol)
}

fn build_file_with_stored_checksum(
    payload: &[Vec<u8>],
    stored: &[u8],
    version: Option<&[u8]>,
    eol: &[u8],
    final_eol: bool,
) -> Vec<u8> {
    assert_eq!(payload.len(), 167);
    let mut lines: Vec<Vec<u8>> = payload.to_vec();
    lines.push(stored.to_vec());
    if let Some(version) = version {
        lines.push(version.to_vec());
    }
    join_lines(&lines, eol, final_eol)
}

fn join_lines(lines: &[Vec<u8>], eol: &[u8], final_eol: bool) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        output.extend_from_slice(line);
        if final_eol || index + 1 < lines.len() {
            output.extend_from_slice(eol);
        }
    }
    output
}

fn v001_lines() -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = V001_LF
        .as_bytes()
        .split(|&byte| byte == b'\n')
        .map(|line| line.to_vec())
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    assert_eq!(lines.len(), 169);
    lines
}

fn v001_payload() -> Vec<Vec<u8>> {
    v001_lines()[..167].to_vec()
}

fn v001_with_replacements(replacements: &[(usize, &[u8])]) -> Vec<u8> {
    let mut payload = v001_payload();
    for &(line, replacement) in replacements {
        payload[line - 1] = replacement.to_vec();
    }
    build_file(&payload, Some(b"20260802"), b"\n", true)
}

fn v001_with(line: usize, replacement: &[u8]) -> Vec<u8> {
    v001_with_replacements(&[(line, replacement)])
}

fn dec(value: i128) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn txt(value: &[u8]) -> Vec<u8> {
    value.to_vec()
}

fn enc(mut value: u128) -> Vec<u8> {
    if value == 0 {
        return b"A".to_vec();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push((value % 10) as u8);
        value /= 10;
    }
    digits.reverse();
    digits
        .into_iter()
        .map(|digit| match digit {
            0 => b'A',
            1 => b'F',
            other => b'0' + other,
        })
        .collect()
}

fn zero_info() -> Vec<Vec<u8>> {
    vec![
        dec(0),
        dec(0),
        dec(0),
        dec(0),
        Vec::new(),
        Vec::new(),
        dec(0),
        Vec::new(),
        Vec::new(),
        dec(0),
        enc(0),
        enc(0),
        dec(0),
        dec(0),
        dec(0),
        dec(0),
    ]
}

struct WireSlot {
    species: i128,
    level: i128,
    sex: i128,
    experience: i128,
    iv: i128,
    held_item: i128,
    selue: i128,
    moves: [i128; 4],
    info: Vec<Vec<u8>>,
}

fn minimal_wire_slot() -> WireSlot {
    WireSlot {
        species: 1,
        level: 1,
        sex: 0,
        experience: 0,
        iv: 0,
        held_item: 0,
        selue: 0,
        moves: [0, 0, 0, 0],
        info: zero_info(),
    }
}

fn empty_wire_slot() -> WireSlot {
    WireSlot {
        species: 0,
        level: 0,
        sex: 0,
        experience: 0,
        iv: 0,
        held_item: 0,
        selue: 0,
        moves: [0, 0, 0, 0],
        info: zero_info(),
    }
}

fn boundary_wire_slot() -> WireSlot {
    WireSlot {
        species: 65535,
        level: 100,
        sex: 2,
        experience: 1000000000,
        iv: 31,
        held_item: 9999,
        selue: 1000000000000,
        moves: [9999, 9999, 9999, 9999],
        info: vec![
            dec(99999),
            dec(100),
            dec(65535),
            dec(255),
            Vec::new(),
            txt(b"Nick\x80"),
            dec(65535),
            txt(b"ts"),
            Vec::new(),
            dec(2147483647),
            enc(4294967290),
            enc(101010),
            dec(-2147483648),
            dec(2147483647),
            dec(9999999999999999),
            dec(2147483647),
        ],
    }
}

fn wire_slot_lines(index: usize, slot: &WireSlot) -> Vec<Vec<u8>> {
    assert_eq!(slot.info.len(), 16);
    let i = index as i128;
    let mut lines = vec![
        dec(slot.species * (5 + i) + 6852),
        dec(slot.level * (37 + i) + 78),
        dec(slot.sex * 5 + 34),
        dec(slot.experience * 3 + 32657),
        dec(slot.iv * 23 + 3127),
        dec(slot.held_item * 43 + 29 * i),
        dec(slot.selue * 7 + 678),
        dec(slot.moves[0] * 13 + 354),
        dec(slot.moves[1] * 17 + 323),
        dec(slot.moves[2] * 19 + 857),
        dec(slot.moves[3] * 29 + 354),
    ];
    lines.extend(slot.info.iter().cloned());
    lines
}

fn rich_payload() -> Vec<Vec<u8>> {
    let mut lines = vec![
        txt(b"Ash\x80\xFF"),
        dec(99999 * 54 + 99),
        dec(10 * 3 + 749104),
        dec(211 * 68 + 53),
        txt(b"caf\xE9"),
    ];
    let slots = [
        minimal_wire_slot(),
        empty_wire_slot(),
        boundary_wire_slot(),
        empty_wire_slot(),
        empty_wire_slot(),
        empty_wire_slot(),
    ];
    for (index, slot) in slots.iter().enumerate() {
        lines.extend(wire_slot_lines(index, slot));
    }
    assert_eq!(lines.len(), 167);
    lines
}

fn rich_crlf_file() -> Vec<u8> {
    build_file(&rich_payload(), Some(b"20260802"), b"\r\n", true)
}

fn rich_lf_file() -> Vec<u8> {
    build_file(&rich_payload(), Some(b"20260802"), b"\n", true)
}

fn profile_r_canonical_v001_payload() -> Vec<Vec<u8>> {
    let mut payload = v001_payload();
    payload[26] = b"A".to_vec();
    payload[27] = b"A".to_vec();
    for slot in 0..5 {
        let start = 33 + 27 * slot - 1;
        for offset in [15, 16, 18, 19] {
            payload[start + offset] = Vec::new();
        }
        for offset in [21, 22] {
            payload[start + offset] = b"A".to_vec();
        }
    }
    payload
}

fn profile_r_canonical_v001_file() -> Vec<u8> {
    build_file(
        &profile_r_canonical_v001_payload(),
        Some(b"20260802"),
        b"\r\n",
        true,
    )
}

fn mixed_line_endings(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut index = 0;
    let mut line = 0;
    while index < input.len() {
        if input[index] == b'\r' && index + 1 < input.len() && input[index + 1] == b'\n' {
            if line % 2 == 0 {
                output.extend_from_slice(b"\r\n");
            } else {
                output.push(b'\n');
            }
            index += 2;
            line += 1;
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
    output
}

fn assert_info_zero(cell: &InfoCell) {
    match cell {
        InfoCell::Numeric(value) => assert_eq!(*value, 0),
        InfoCell::Text(value) => assert!(value.is_empty()),
        InfoCell::EncodedNumeric(value) => assert_eq!(*value, 0),
    }
}

fn assert_info_numeric(cell: &InfoCell, expected: i64) {
    match cell {
        InfoCell::Numeric(value) => assert_eq!(*value, expected),
        other => panic!("expected numeric info cell, got {other:?}"),
    }
}

fn assert_info_text(cell: &InfoCell, expected: &[u8]) {
    match cell {
        InfoCell::Text(value) => assert_eq!(value.as_slice(), expected),
        other => panic!("expected text info cell, got {other:?}"),
    }
}

fn assert_info_encoded(cell: &InfoCell, expected: u64) {
    match cell {
        InfoCell::EncodedNumeric(value) => assert_eq!(*value, expected),
        other => panic!("expected encoded numeric info cell, got {other:?}"),
    }
}

#[test]
fn golden_v001_layout_and_checksum() {
    let lines = v001_lines();
    assert_eq!(lines.len(), 169);
    assert_eq!(V001_LF.len(), 497);
    assert_eq!(lines[167].as_slice(), b"2876568");
    assert_eq!(lines[168].as_slice(), b"20260802");
    assert_eq!(reference_checksum_lines(&v001_payload()), 2876568);

    let payload = v001_payload();
    assert_eq!(
        build_file(&payload, Some(b"20260802"), b"\n", true),
        V001_LF.as_bytes()
    );
    assert_eq!(
        build_file(&payload, Some(b"20260802"), b"\r\n", true).len(),
        666
    );
    assert_eq!(build_file(&payload, None, b"\n", true).len(), 488);
    assert_eq!(build_file(&payload, None, b"\n", false).len(), 487);
}

#[test]
fn golden_v001_decoded_values() {
    let result = decode_default(V001_LF.as_bytes()).expect("V-001 must decode");
    assert_eq!(result.version, TEST_CURRENT_VERSION);
    let team = &result.team;
    assert_eq!(team.slots.len(), 6);
    assert_eq!(team.trainer.name.as_slice(), b"A");
    assert_eq!(team.trainer.id, 0);
    assert_eq!(team.trainer.index, 0);
    assert_eq!(team.trainer.outfit, 0);
    assert!(team.trainer.phrase.is_empty());

    assert_eq!(team.slots[0].species, 1);
    assert_eq!(team.slots[0].level, 1);
    assert_eq!(team.slots[0].sex, 1);
    assert_eq!(team.slots[0].experience, 0);
    assert_eq!(team.slots[0].iv, 0);
    assert_eq!(team.slots[0].held_item, 0);
    assert_eq!(team.slots[0].selue, 0);
    assert_eq!(team.slots[0].moves, [0, 0, 0, 0]);
    for cell in &team.slots[0].info {
        assert_info_zero(cell);
    }

    for slot in &team.slots[1..] {
        assert_eq!(slot.species, 0);
        assert_eq!(slot.level, 0);
        assert_eq!(slot.sex, 0);
        assert_eq!(slot.experience, 0);
        assert_eq!(slot.iv, 0);
        assert_eq!(slot.held_item, 0);
        assert_eq!(slot.selue, 0);
        assert_eq!(slot.moves, [0, 0, 0, 0]);
        for cell in &slot.info {
            assert_info_zero(cell);
        }
    }
}

#[test]
fn golden_checksum_vectors() {
    let cases: &[(&[u8], u64)] = &[
        (b"", 19990),
        (b"1", 16546),
        (b"Hello", 9592),
        (b"Hello, World!", 45124),
    ];
    for &(secret, expected) in cases {
        let lines = secret_lines(secret);
        assert_eq!(reference_checksum_lines(&lines), expected);
        assert_eq!(codec_checksum(&lines), expected);
    }

    let b221 = vec![b'B'; 221];
    let b229 = vec![b'B'; 229];
    assert_eq!(codec_checksum(&secret_lines(&b221)), 1737864);
    assert_eq!(codec_checksum(&secret_lines(&b229)), 1839735);
    assert_eq!(codec_checksum(&v001_payload()), 2876568);
    assert_eq!(
        codec_checksum(&rich_payload()),
        reference_checksum_lines(&rich_payload())
    );
}

fn secret_lines(secret: &[u8]) -> Vec<Vec<u8>> {
    let mut lines = Vec::with_capacity(167);
    lines.push(secret.to_vec());
    for _ in 1..167 {
        lines.push(Vec::new());
    }
    lines
}

#[test]
fn checksum_rejects_wrong_line_count() {
    let lines = secret_lines(b"Hello");
    let short: Vec<&[u8]> = lines[..166].iter().map(|line| line.as_slice()).collect();
    assert!(checksum(&short).is_err());

    let mut long_lines = lines;
    long_lines.push(Vec::new());
    let long: Vec<&[u8]> = long_lines.iter().map(|line| line.as_slice()).collect();
    assert!(checksum(&long).is_err());
}

#[test]
fn canonicalize_v001_uses_profile_r_canonical_form() {
    let expected = profile_r_canonical_v001_file();
    let canonical = canonicalize(V001_LF.as_bytes(), &DecodeOptions::default())
        .expect("V-001 must canonicalize");
    assert_eq!(canonical, expected);
    assert_eq!(
        canonicalize(&canonical, &DecodeOptions::default()).expect("canonical output must decode"),
        expected
    );

    let lines = split_lines(&canonical);
    assert_eq!(lines.len(), 169);
    assert_eq!(lines[167].as_ref(), b"2546350");
    assert_eq!(lines[168].as_ref(), b"20260802");
    for one_based in [27, 28, 54, 55, 81, 82, 108, 109, 135, 136, 162, 163] {
        assert_eq!(lines[one_based - 1].as_ref(), b"A");
    }
    for one_based in [
        48, 49, 51, 52, 75, 76, 78, 79, 102, 103, 105, 106, 129, 130, 132, 133, 156, 157, 159, 160,
    ] {
        assert!(lines[one_based - 1].is_empty());
    }
    for one_based in [21, 22, 24, 25] {
        assert_eq!(lines[one_based - 1].as_ref(), b"0");
    }

    let decoded = decode_strict(&canonical).expect("Profile R canonical form must be strict");
    assert_eq!(decoded.version, TEST_CURRENT_VERSION);
}

#[test]
fn line_ending_and_terminal_variants_are_accepted() {
    let payload = v001_payload();
    let lf = build_file(&payload, Some(b"20260802"), b"\n", true);
    let crlf = build_file(&payload, Some(b"20260802"), b"\r\n", true);
    let no_final = build_file(&payload, Some(b"20260802"), b"\n", false);
    let no_version = build_file(&payload, None, b"\n", true);
    let no_version_no_final = build_file(&payload, None, b"\n", false);

    for input in [&lf, &crlf, &no_final, &no_version, &no_version_no_final] {
        let decoded = decode_default(input).expect("line-ending variant must decode");
        assert_eq!(decoded.team.trainer.name.as_slice(), b"A");
        assert_eq!(decoded.team.slots[0].species, 1);
    }
    assert_eq!(
        decode_default(&no_version)
            .expect("missing version must decode")
            .version,
        TEST_DEFAULT_VERSION
    );
    assert_eq!(
        decode_default(&no_version_no_final)
            .expect("missing version without final newline must decode")
            .version,
        TEST_DEFAULT_VERSION
    );

    let mut trailing = crlf.clone();
    trailing.extend_from_slice(b"\r\n\r\n");
    let decoded = decode_default(&trailing).expect("trailing empty lines must be ignored");
    assert_eq!(decoded.version, TEST_CURRENT_VERSION);

    let mixed = mixed_line_endings(&crlf);
    let decoded = decode_default(&mixed).expect("mixed line endings must decode");
    assert_eq!(decoded.version, TEST_CURRENT_VERSION);
    assert!(decode_strict(&mixed).is_err());

    let cr_only: Vec<u8> = lf
        .iter()
        .map(|&byte| if byte == b'\n' { b'\r' } else { byte })
        .collect();
    assert_decode_category(&cr_only, ErrorCategory::TruncatedBody);
}

#[test]
fn noncanonical_encoded_info_is_rejected_in_strict_mode() {
    let v001_crlf = build_file(&v001_payload(), Some(b"20260802"), b"\r\n", true);
    let decoded = decode_default(&v001_crlf).expect("legacy plain zero must decode");
    assert_info_encoded(&decoded.team.slots[0].info[10], 0);
    assert_info_encoded(&decoded.team.slots[0].info[11], 0);
    assert!(decode_strict(&v001_crlf).is_err());
}

#[test]
fn rich_canonical_round_trip() {
    let bytes = rich_crlf_file();
    let decoded = decode_default(&bytes).expect("rich canonical team must decode");
    let encoded = encode(&decoded.team, &EncodeOptions::default()).expect("team must encode");
    assert_eq!(encoded.bytes, bytes);
    assert_eq!(encoded.version, TEST_CURRENT_VERSION);

    let strict = decode_strict(&bytes).expect("rich canonical team must pass strict decode");
    let strict_encoded =
        encode(&strict.team, &EncodeOptions::default()).expect("strict team must encode");
    assert_eq!(strict_encoded.bytes, bytes);
}

#[test]
fn rich_team_decoded_values() {
    let bytes = rich_crlf_file();
    let result = decode_default(&bytes).expect("rich canonical team must decode");
    let team = &result.team;
    assert_eq!(result.version, TEST_CURRENT_VERSION);
    assert_eq!(team.slots.len(), 6);
    assert_eq!(team.trainer.name.as_slice(), b"Ash\x80\xFF".as_slice());
    assert_eq!(team.trainer.id, 99999);
    assert_eq!(team.trainer.index, 10);
    assert_eq!(team.trainer.outfit, 211);
    assert_eq!(team.trainer.phrase.as_slice(), b"caf\xE9".as_slice());

    let first = &team.slots[0];
    assert_eq!(first.species, 1);
    assert_eq!(first.level, 1);
    assert_eq!(first.sex, 0);
    assert_eq!(first.experience, 0);
    assert_eq!(first.iv, 0);
    assert_eq!(first.held_item, 0);
    assert_eq!(first.selue, 0);
    assert_eq!(first.moves, [0, 0, 0, 0]);
    for cell in &first.info {
        assert_info_zero(cell);
    }

    let empty = &team.slots[1];
    assert_eq!(empty.species, 0);
    assert_eq!(empty.level, 0);
    assert_eq!(empty.sex, 0);
    assert_eq!(empty.experience, 0);
    assert_eq!(empty.iv, 0);
    assert_eq!(empty.held_item, 0);
    assert_eq!(empty.selue, 0);
    assert_eq!(empty.moves, [0, 0, 0, 0]);
    for cell in &empty.info {
        assert_info_zero(cell);
    }

    let boundary = &team.slots[2];
    assert_eq!(boundary.species, 65535);
    assert_eq!(boundary.level, 100);
    assert_eq!(boundary.sex, 2);
    assert_eq!(boundary.experience, 1000000000);
    assert_eq!(boundary.iv, 31);
    assert_eq!(boundary.held_item, 9999);
    assert_eq!(boundary.selue, 1000000000000);
    assert_eq!(boundary.moves, [9999, 9999, 9999, 9999]);
    assert_info_numeric(&boundary.info[0], 99999);
    assert_info_numeric(&boundary.info[1], 100);
    assert_info_numeric(&boundary.info[2], 65535);
    assert_info_numeric(&boundary.info[3], 255);
    assert_info_text(&boundary.info[4], b"");
    assert_info_text(&boundary.info[5], b"Nick\x80");
    assert_info_numeric(&boundary.info[6], 65535);
    assert_info_text(&boundary.info[7], b"ts");
    assert_info_text(&boundary.info[8], b"");
    assert_info_numeric(&boundary.info[9], 2147483647);
    assert_info_encoded(&boundary.info[10], 4294967290);
    assert_info_encoded(&boundary.info[11], 101010);
    assert_info_numeric(&boundary.info[12], -2147483648);
    assert_info_numeric(&boundary.info[13], 2147483647);
    assert_info_numeric(&boundary.info[14], 9999999999999999);
    assert_info_numeric(&boundary.info[15], 2147483647);

    for slot in &team.slots[3..] {
        assert_eq!(slot.species, 0);
        for cell in &slot.info {
            assert_info_zero(cell);
        }
    }
}

#[test]
fn accepted_noncanonical_inputs_canonicalize_identically() {
    let canonical = rich_crlf_file();
    let lf = rich_lf_file();
    let mut no_final = canonical.clone();
    no_final.truncate(no_final.len() - 2);
    let mut trailing = canonical.clone();
    trailing.extend_from_slice(b"\r\n\r\n");
    let mixed = mixed_line_endings(&canonical);

    for input in [&lf, &no_final, &trailing, &mixed] {
        let once = canonicalize(input, &DecodeOptions::default())
            .expect("compatible input must canonicalize");
        assert_eq!(once, canonical);
        let twice = canonicalize(&once, &DecodeOptions::default())
            .expect("canonical output must canonicalize");
        assert_eq!(twice, once);
    }

    let no_version = build_file(&rich_payload(), None, b"\n", true);
    let canonical_no_version = canonicalize(&no_version, &DecodeOptions::default())
        .expect("input without version must canonicalize");
    assert_eq!(canonical_no_version, canonical);
}

#[test]
fn malformed_container_and_checksum() {
    assert_decode_category(b"", ErrorCategory::InputEmpty);

    let v001 = v001_lines();
    let short = join_lines(&v001[..100], b"\n", true);
    assert_decode_category(&short, ErrorCategory::TruncatedBody);

    let mismatch = build_file_with_stored_checksum(
        &v001_payload(),
        b"2876569",
        Some(b"20260802"),
        b"\n",
        true,
    );
    assert_decode_category(&mismatch, ErrorCategory::ChecksumMismatch);

    let bad_syntax =
        build_file_with_stored_checksum(&v001_payload(), b"abc", Some(b"20260802"), b"\n", true);
    assert_decode_category(&bad_syntax, ErrorCategory::ChecksumSyntax);

    let empty_checksum =
        build_file_with_stored_checksum(&v001_payload(), b"", Some(b"20260802"), b"\n", true);
    assert_decode_category(&empty_checksum, ErrorCategory::ChecksumSyntax);

    let mut malformed_payload = v001_payload();
    malformed_payload[5] = b"abc".to_vec();
    let checksum_precedes_fields = build_file_with_stored_checksum(
        &malformed_payload,
        b"2876568",
        Some(b"20260802"),
        b"\n",
        true,
    );
    assert_decode_category(&checksum_precedes_fields, ErrorCategory::ChecksumMismatch);
}

#[test]
fn malformed_numeric_fields() {
    assert_decode_category(&v001_with(6, b"abc"), ErrorCategory::ExpectedNumber);
    assert_decode_category(&v001_with(6, b""), ErrorCategory::ExpectedNumber);
    assert_decode_category(&v001_with(6, b"6,857"), ErrorCategory::ExpectedNumber);
    assert_decode_category(&v001_with(6, b"6857.5"), ErrorCategory::NotInteger);
    assert_decode_category(&v001_with(2, b"100"), ErrorCategory::NotInteger);
}

#[test]
fn malformed_text_fields() {
    assert_decode_category(&v001_with(1, b"A\tB"), ErrorCategory::TextControl);
    assert_decode_category(&v001_with(5, b"A\x00B"), ErrorCategory::TextControl);
    assert_decode_category(&v001_with(21, b"A\x01B"), ErrorCategory::TextControl);

    let long_name = vec![b'A'; 65];
    assert_decode_category(&v001_with(1, &long_name), ErrorCategory::TextTooLong);
    let long_phrase = vec![b'P'; 257];
    assert_decode_category(&v001_with(5, &long_phrase), ErrorCategory::TextTooLong);
    let long_info = vec![b'I'; 257];
    assert_decode_category(&v001_with(21, &long_info), ErrorCategory::TextTooLong);

    assert_decode_category(&v001_with(1, b""), ErrorCategory::NameEmpty);
}

#[test]
fn malformed_semantic_fields() {
    let empty_slot = [
        (6, b"6852".as_slice()),
        (7, b"78".as_slice()),
        (8, b"34".as_slice()),
        (9, b"32657".as_slice()),
        (10, b"3127".as_slice()),
        (11, b"0".as_slice()),
        (12, b"678".as_slice()),
        (13, b"354".as_slice()),
        (14, b"323".as_slice()),
        (15, b"857".as_slice()),
        (16, b"354".as_slice()),
    ];
    assert_decode_category(
        &v001_with_replacements(&empty_slot),
        ErrorCategory::NoPokemon,
    );

    assert_decode_category(&v001_with(7, b"78"), ErrorCategory::LevelInvalid);
    assert_decode_category(&v001_with(8, b"49"), ErrorCategory::SexInvalid);
    assert_decode_category(&v001_with(10, b"230003127"), ErrorCategory::IvInvalid);
    assert_decode_category(&v001_with(11, b"2818048"), ErrorCategory::ItemInvalid);
    assert_decode_category(
        &v001_with(9, b"6442483601"),
        ErrorCategory::ExperienceInvalid,
    );
    assert_decode_category(
        &v001_with(12, b"70000000000000678"),
        ErrorCategory::SelueInvalid,
    );
    assert_decode_category(&v001_with(13, b"852322"), ErrorCategory::MoveInvalid);

    assert!(decode_default(&v001_with(6, b"6847")).is_err());
}

#[test]
fn malformed_header_ranges() {
    assert_decode_category(&v001_with(2, b"54000099"), ErrorCategory::NumberOutOfRange);
    assert_decode_category(&v001_with(3, b"749872"), ErrorCategory::NumberOutOfRange);
    assert_decode_category(&v001_with(4, b"68000053"), ErrorCategory::NumberOutOfRange);
}

#[test]
fn malformed_encoded_info_and_bom() {
    assert_decode_category(&v001_with(27, b"12Z"), ErrorCategory::InfoEncodingInvalid);

    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(V001_LF.as_bytes());
    assert!(decode_default(&bom).is_err());
}

#[test]
fn version_acceptance_and_default() {
    let payload = v001_payload();
    let accepted = [
        (None, TEST_DEFAULT_VERSION),
        (Some(&b"20080101"[..]), 20080101),
        (Some(&b"20240229"[..]), 20240229),
        (Some(&b"20240713"[..]), 20240713),
        (Some(&b"20260802"[..]), TEST_CURRENT_VERSION),
        (Some(&b"20260803"[..]), 20260803),
        (Some(&b"20991231"[..]), 20991231),
        (Some(&b"21001231"[..]), 21001231),
    ];
    for (version, expected) in accepted {
        let bytes = build_file(&payload, version, b"\n", true);
        let decoded = decode_default(&bytes).expect("supported version must decode");
        assert_eq!(decoded.version, expected);
        assert_eq!(decoded.team.slots[0].species, 1);
    }
}

#[test]
fn version_rejections() {
    let payload = v001_payload();
    for version in [
        &b"20230229"[..],
        &b"21000229"[..],
        &b"20241301"[..],
        &b"20240230"[..],
    ] {
        let bytes = build_file(&payload, Some(version), b"\n", true);
        assert_decode_category(&bytes, ErrorCategory::VersionDate);
    }
    for version in [&b"20071231"[..], &b"21010101"[..]] {
        let bytes = build_file(&payload, Some(version), b"\n", true);
        assert_decode_category(&bytes, ErrorCategory::VersionUnsupported);
    }
    for version in [
        &b"abc"[..],
        &b"20240713.5"[..],
        &b"2024071"[..],
        &b"202407130"[..],
        &b"2024-07-13"[..],
    ] {
        let bytes = build_file(&payload, Some(version), b"\n", true);
        assert_decode_category(&bytes, ErrorCategory::VersionSyntax);
    }
}

#[test]
fn version_line_is_outside_checksum() {
    let payload = v001_payload();
    for version in [&b"20240713"[..], &b"20260803"[..], &b"20991231"[..]] {
        let bytes = build_file(&payload, Some(version), b"\n", true);
        let lines = split_lines(&bytes);
        assert_eq!(lines[167].as_ref(), b"2876568");
        let decoded = decode_default(&bytes).expect("version change must not invalidate checksum");
        assert_eq!(decoded.team.slots[0].species, 1);
    }
}

#[test]
fn legacy_outfit_migration() {
    let mut payload = v001_payload();
    payload[2] = b"749134".to_vec();
    payload[3] = b"14401".to_vec();
    assert_eq!(reference_checksum_lines(&payload), 2898436);

    let old = build_file(&payload, Some(b"20240713"), b"\n", true);
    let old_team = decode_default(&old).expect("legacy file must decode");
    assert_eq!(old_team.team.trainer.index, 10);
    assert_eq!(old_team.team.trainer.outfit, 212);

    let new = build_file(
        &payload,
        Some(TEST_CUTOVER_VERSION.to_string().as_bytes()),
        b"\n",
        true,
    );
    let new_team = decode_default(&new).expect("cutover file must decode");
    assert_eq!(new_team.team.trainer.index, 10);
    assert_eq!(new_team.team.trainer.outfit, 211);
}
