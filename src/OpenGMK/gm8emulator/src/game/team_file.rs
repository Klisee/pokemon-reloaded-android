use std::fmt;

pub(crate) const SECRET_LINES: usize = 167;
pub(crate) const CURRENT_VERSION: i32 = 20260802;
pub(crate) const DEFAULT_VERSION: i32 = 20240713;

const HEADER_LINES: usize = 5;
const SLOT_LINES: usize = 27;
const INFO_LINES: usize = 16;
const PARTY_SIZE: usize = 6;

const NAME_LIMIT: usize = 64;
const PHRASE_LIMIT: usize = 256;
const INFO_TEXT_LIMIT: usize = 256;

#[derive(Clone, Debug)]
pub(crate) struct TeamFile {
    pub name: Vec<u8>,
    pub trainer_id: i32,
    pub trainer_index: i32,
    pub outfit: f64,
    pub phrase: Vec<u8>,
    pub mons: [TeamMon; PARTY_SIZE],
    pub version: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct TeamMon {
    pub species: i32,
    pub level: i32,
    pub sex: i32,
    pub exp: f64,
    pub iv: i32,
    pub item: i32,
    pub selue: f64,
    pub moves: [f64; 4],
    pub info: [Vec<u8>; INFO_LINES],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TeamError(pub String);

impl fmt::Display for TeamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TeamError {}

impl TeamFile {
    pub(crate) fn decode(data: &[u8]) -> Result<Self, TeamError> {
        let lines = split_lines(data);
        if lines.len() < SECRET_LINES + 1 {
            return Err(error(format!(
                "equipo incompleto: {} líneas, se esperaban al menos {}",
                lines.len(),
                SECRET_LINES + 1
            )))
        }

        let secret = lines[..SECRET_LINES].iter().flat_map(|line| line.iter().copied()).collect::<Vec<_>>();
        let expected = checksum(&secret);
        let loaded = integer(lines[SECRET_LINES], "checksum")?;
        if loaded != expected {
            eprintln!(
                "[team] checksum loaded={loaded} expected={expected} lines={} bytes={}",
                lines.len(),
                data.len()
            );
            let hex: String = data.iter().map(|byte| format!("{byte:02x}")).collect();
            for (index, chunk) in hex.as_bytes().chunks(800).enumerate() {
                eprintln!("[team] hex {index} {}", std::str::from_utf8(chunk).unwrap_or(""));
            }
            return Err(error(format!("checksum inválido: {loaded}, se esperaba {expected}")))
        }

        let version = match lines.get(SECRET_LINES + 1) {
            Some(line) if !line.is_empty() => integer(line, "versión")? as i32,
            _ => DEFAULT_VERSION,
        };
        validate_version(version)?;

        let mut team = Self {
            name: lines[0].to_vec(),
            trainer_id: 0,
            trainer_index: 0,
            outfit: 0.0,
            phrase: lines[4].to_vec(),
            mons: std::array::from_fn(|_| TeamMon::empty()),
            version,
        };

        team.trainer_id = ((integer(lines[1], "prota_ID")? - 99) / 54) as i32;
        team.trainer_index = ((integer(lines[2], "prota_index")? - 749_104) / 3) as i32;
        team.outfit = (number(lines[3], "outfit")? - 53.0) / 68.0;

        let mut cursor = HEADER_LINES;
        for slot in 0..PARTY_SIZE {
            let i = slot as f64;
            let species = integer(lines[cursor], "especie")? as i32;
            let level = integer(lines[cursor + 1], "nivel")? as i32;
            let sex = integer(lines[cursor + 2], "sexo")? as i32;
            let exp = number(lines[cursor + 3], "EXP")?;
            let iv = integer(lines[cursor + 4], "IV")? as i32;
            let item = integer(lines[cursor + 5], "objeto")? as i32;
            let selue = number(lines[cursor + 6], "selue")?;

            let mut moves = [0.0; 4];
            for (move_slot, move_index) in (1..=4u32).enumerate() {
                let (factor, offset) = move_encoding(move_index);
                moves[move_slot] = (number(lines[cursor + 7 + move_slot], "ataque")? - offset) / factor;
            }

            let info_start = cursor + 11;
            let mut info: [Vec<u8>; INFO_LINES] = std::array::from_fn(|_| Vec::new());
            for field in 0..INFO_LINES {
                info[field] = lines[info_start + field].to_vec();
            }

            team.mons[slot] = TeamMon {
                species: decode_value(f64::from(species), 6_852.0, 5.0 + i)? as i32,
                level: decode_value(f64::from(level), 78.0, 37.0 + i)? as i32,
                sex: decode_value(f64::from(sex), 34.0, 5.0)? as i32,
                exp: decode_value(exp, 32_657.0, 3.0)?,
                iv: decode_value(f64::from(iv), 3_127.0, 23.0)? as i32,
                item: decode_value(f64::from(item), 29.0 * i, 43.0)? as i32,
                selue: decode_value(selue, 678.0, 7.0)?,
                moves,
                info,
            };
            cursor += SLOT_LINES;
        }

        team.validate()?;
        Ok(team)
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut lines: Vec<Vec<u8>> = Vec::with_capacity(SECRET_LINES + 2);
        let mut secret = Vec::new();
        let mut push = |line: Vec<u8>| {
            secret.extend_from_slice(&line);
            lines.push(line);
        };

        push(self.name.clone());
        push(number_text(f64::from(self.trainer_id) * 54.0 + 99.0));
        push(number_text(f64::from(self.trainer_index) * 3.0 + 749_104.0));
        push(number_text(self.outfit * 68.0 + 53.0));
        push(self.phrase.clone());

        for (slot, mon) in self.mons.iter().enumerate() {
            let i = slot as f64;
            push(number_text(f64::from(mon.species) * (5.0 + i) + 6_852.0));
            push(number_text(f64::from(mon.level) * (37.0 + i) + 78.0));
            push(number_text(f64::from(mon.sex) * 5.0 + 34.0));
            push(number_text(mon.exp * 3.0 + 32_657.0));
            push(number_text(f64::from(mon.iv) * 23.0 + 3_127.0));
            push(number_text(f64::from(mon.item) * 43.0 + 29.0 * i));
            push(number_text(mon.selue * 7.0 + 678.0));
            for (move_slot, value) in mon.moves.iter().enumerate() {
                let (factor, offset) = move_encoding((move_slot + 1) as u32);
                push(number_text(value * factor + offset));
            }
            for line in &mon.info {
                push(line.clone());
            }
        }

        debug_assert_eq!(lines.len(), SECRET_LINES);
        lines.push(checksum(&secret).to_string().into_bytes());
        lines.push(self.version.to_string().into_bytes());

        let mut data = Vec::new();
        for line in lines {
            data.extend_from_slice(&line);
            data.extend_from_slice(b"\r\n");
        }
        data
    }

    pub(crate) fn validate(&self) -> Result<(), TeamError> {
        if self.name.is_empty() {
            return Err(error("el nombre del entrenador está vacío"))
        }
        validate_text(&self.name, "nombre", NAME_LIMIT)?;
        validate_text(&self.phrase, "frase", PHRASE_LIMIT)?;
        if !(0..=999_999).contains(&self.trainer_id) {
            return Err(error("prota_ID fuera de rango"))
        }
        if !(0..=10).contains(&self.trainer_index) {
            return Err(error("prota_index fuera de rango"))
        }
        let outfit_limit = if self.trainer_index == 10 { 10_000.0 } else { 999_999.0 };
        if !self.outfit.is_finite() || self.outfit < -1.0 || self.outfit > outfit_limit {
            return Err(error("outfit fuera de rango"))
        }
        validate_version(self.version)?;

        let mut occupied = 0usize;
        for (slot, mon) in self.mons.iter().enumerate() {
            let label = slot + 1;
            if mon.species == 0 {
                continue
            }
            occupied += 1;
            if !(1..=999_999).contains(&mon.species) {
                return Err(error(format!("slot {label}: especie fuera de rango")))
            }
            if !(1..=9_999).contains(&mon.level) {
                return Err(error(format!("slot {label}: nivel fuera de rango")))
            }
            if !(0..=2).contains(&mon.sex) {
                return Err(error(format!("slot {label}: sexo fuera de rango")))
            }
            if !mon.exp.is_finite() || !(0.0..=1_000_000_000.0).contains(&mon.exp) {
                return Err(error(format!("slot {label}: EXP fuera de rango")))
            }
            if !(0..=9_999_999).contains(&mon.iv) {
                return Err(error(format!("slot {label}: IV fuera de rango")))
            }
            if !(0..=9_999).contains(&mon.item) {
                return Err(error(format!("slot {label}: objeto fuera de rango")))
            }
            if !mon.selue.is_finite() || mon.selue.abs() > 9_007_199_254_740_991.0 {
                return Err(error(format!("slot {label}: selue fuera de rango")))
            }
            for (move_slot, value) in mon.moves.iter().enumerate() {
                if !value.is_finite() || !(0.0..=9_999.0).contains(value) {
                    return Err(error(format!("slot {label}: ataque {} fuera de rango", move_slot + 1)))
                }
            }
            for (field, line) in mon.info.iter().enumerate() {
                validate_text(line, &format!("slot {label} info[{field}]"), INFO_TEXT_LIMIT)?;
            }
        }
        if occupied == 0 {
            return Err(error("el equipo no contiene ningún Pokémon"))
        }
        Ok(())
    }
}

impl TeamMon {
    pub(crate) fn empty() -> Self {
        Self {
            species: 0,
            level: 0,
            sex: 0,
            exp: 0.0,
            iv: 0,
            item: 0,
            selue: 0.0,
            moves: [0.0; 4],
            info: std::array::from_fn(|_| Vec::new()),
        }
    }
}

pub(crate) fn checksum(secret: &[u8]) -> i64 {
    let mut total: i64 = 0;
    let mut cycle = 0usize;
    for k in 0..=secret.len() {
        let ord = if secret.is_empty() {
            0
        } else if k == 0 {
            i64::from(secret[0])
        } else {
            i64::from(secret[k - 1])
        };
        let k = k as i64;
        let add = match cycle {
            0 => ord * 7 + k + 9,
            1 => ord * 3 + 2 * k,
            2 => ord * 5 + k + (k + 2) / 3,
            3 => ord * 9 + k + 53,
            4 => ord * 15 + 3 * k,
            5 => (ord * 3 + k) ^ 2,
            6 => ord * 7 + 37 * k,
            _ => ord * 9 + k + crate::math::ieee_round(k as f64 / 2.0) as i64,
        };
        total = total.wrapping_add(add);
        cycle = (cycle + 1) % 8;
    }
    total.wrapping_mul(7).wrapping_sub(20_053).abs()
}

pub(crate) fn encode_number_text(value: &[u8]) -> Vec<u8> {
    value
        .iter()
        .map(|byte| match byte {
            b'0' => b'A',
            b'1' => b'F',
            _ => *byte,
        })
        .collect()
}

pub(crate) fn decode_number_text(value: &[u8]) -> Vec<u8> {
    value
        .iter()
        .map(|byte| match byte {
            b'A' | b'E' | b'I' | b'O' | b'U' | b'Y' => b'0',
            b'F' | b'R' | b'N' | b'D' | b'S' | b'G' | b'V' | b'L' => b'1',
            _ => *byte,
        })
        .collect()
}

pub(crate) fn line_number(line: &[u8]) -> f64 {
    std::str::from_utf8(line)
        .ok()
        .and_then(|text| text.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

pub(crate) fn number_text(value: f64) -> Vec<u8> {
    if value.fract() == 0.0 {
        format!("{value:.0}").into_bytes()
    } else {
        format!("{value:.2}").into_bytes()
    }
}

pub(crate) fn number(line: &[u8], field: &str) -> Result<f64, TeamError> {
    let text = std::str::from_utf8(line).map_err(|_| error(format!("{field}: no es UTF-8 válido")))?;
    let value = text.trim().parse::<f64>().map_err(|_| error(format!("{field}: número inválido")))?;
    if !value.is_finite() {
        return Err(error(format!("{field}: NaN o infinito")))
    }
    Ok(value)
}

fn integer(line: &[u8], field: &str) -> Result<i64, TeamError> {
    let value = number(line, field)?;
    if value.fract().abs() > 1e-9 {
        return Err(error(format!("{field}: se esperaba un entero")))
    }
    if value < -9_007_199_254_740_992.0 || value > 9_007_199_254_740_992.0 {
        return Err(error(format!("{field}: entero fuera de rango")))
    }
    Ok(value.round() as i64)
}

fn decode_value(value: f64, offset: f64, factor: f64) -> Result<f64, TeamError> {
    let result = (value - offset) / factor;
    if !result.is_finite() {
        return Err(error("valor decodificado no finito"))
    }
    Ok(result)
}

fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut lines: Vec<&[u8]> = data.split(|byte| *byte == b'\n').collect();
    for line in &mut lines {
        if line.last() == Some(&b'\r') {
            *line = &line[..line.len() - 1];
        }
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn validate_version(version: i32) -> Result<(), TeamError> {
    if !(20_080_101..=21_001_231).contains(&version) {
        return Err(error("versión fuera de rango"))
    }
    let year = version / 10_000;
    let month = (version / 100) % 100;
    let day = version % 100;
    if !(2008..=2100).contains(&year) || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return Err(error("versión con fecha inválida"))
    }
    Ok(())
}

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn validate_text(line: &[u8], field: &str, limit: usize) -> Result<(), TeamError> {
    if line.len() > limit {
        return Err(error(format!("{field}: texto demasiado largo")))
    }
    if line.iter().any(|byte| *byte == 0 || *byte < 0x20 || *byte == 0x7f) {
        return Err(error(format!("{field}: contiene un carácter de control")))
    }
    Ok(())
}

fn move_encoding(slot: u32) -> (f64, f64) {
    match slot {
        1 => (13.0, 354.0),
        2 => (17.0, 323.0),
        3 => (19.0, 857.0),
        _ => (29.0, 354.0),
    }
}

fn error(message: impl Into<String>) -> TeamError {
    TeamError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TeamFile {
        let mut mons = std::array::from_fn(|_| TeamMon::empty());
        for (slot, mon) in mons.iter_mut().enumerate() {
            let i = slot as f64;
            *mon = TeamMon {
                species: 1 + slot as i32,
                level: 5 + slot as i32,
                sex: 1,
                exp: 100.0,
                iv: 555_555,
                item: 0,
                selue: (9_000.0 - (1.0 + i)) * 1_000_000.0 + 555_555.0,
                moves: [1.0, 2.0, 3.0, 4.0],
                info: std::array::from_fn(|field| if field == 4 { b"Ash".to_vec() } else { b"0".to_vec() }),
            };
        }
        TeamFile {
            name: b"Ash".to_vec(),
            trainer_id: 1,
            trainer_index: 1,
            outfit: 1.0,
            phrase: b"Hello".to_vec(),
            mons,
            version: CURRENT_VERSION,
        }
    }

    #[test]
    fn round_trip_preserves_team() {
        let original = sample();
        let decoded = TeamFile::decode(&original.encode()).expect("decode");
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.trainer_id, original.trainer_id);
        assert_eq!(decoded.trainer_index, original.trainer_index);
        assert_eq!(decoded.mons[0].species, original.mons[0].species);
        assert_eq!(decoded.mons[5].moves, original.mons[5].moves);
        assert_eq!(decoded.mons[3].info[4], original.mons[3].info[4]);
        assert_eq!(decoded.version, original.version);
    }

    #[test]
    fn rejects_truncated_file() {
        let data = sample().encode();
        assert!(TeamFile::decode(&data[..data.len() / 2]).is_err());
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut data = sample().encode();
        let index = data.len() / 2;
        data[index] = if data[index] == b'0' { b'1' } else { b'0' };
        assert!(TeamFile::decode(&data).is_err());
    }

    #[test]
    fn rejects_non_finite_and_out_of_range() {
        let mut team = sample();
        team.mons[0].exp = f64::NAN;
        assert!(team.validate().is_err());

        let mut team = sample();
        team.mons[0].level = 10_000;
        assert!(team.validate().is_err());

        let mut team = sample();
        team.mons[0].info[0] = b"bad\0value".to_vec();
        assert!(team.validate().is_err());
    }

    #[test]
    fn checksum_vectors_match_protocol() {
        assert_eq!(checksum(b""), 19_990);
        assert_eq!(checksum(b"1"), 16_546);
        assert_eq!(checksum(b"Hello"), 9_592);
        assert_eq!(checksum(b"Hello, World!"), 45_124);
        assert_eq!(checksum(&vec![b'B'; 221]), 1_737_864);
        assert_eq!(checksum(&vec![b'B'; 229]), 1_839_735);
    }
}
