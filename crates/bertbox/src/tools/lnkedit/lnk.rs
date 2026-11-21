//! Windows shortcut (`.lnk`) files, per MS-SHLLINK. Only the target, arguments, and working
//! directory are interpreted; everything else is carried through unchanged.
use std::fmt;

const HEADER_SIZE: u32 = 0x4C;
const CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];
const FLAGS_OFFSET: usize = 0x14;

const HAS_LINK_TARGET_ID_LIST: u32 = 1 << 0;
const HAS_LINK_INFO: u32 = 1 << 1;
const HAS_NAME: u32 = 1 << 2;
const HAS_RELATIVE_PATH: u32 = 1 << 3;
const HAS_WORKING_DIR: u32 = 1 << 4;
const HAS_ARGUMENTS: u32 = 1 << 5;
const HAS_ICON_LOCATION: u32 = 1 << 6;
const IS_UNICODE: u32 = 1 << 7;
const HAS_EXP_STRING: u32 = 1 << 9;

const LINK_INFO_HEADER_SIZE_UNICODE: u32 = 0x24;
const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 1 << 0;
const COMMON_NETWORK_RELATIVE_LINK_AND_PATH_SUFFIX: u32 = 1 << 1;
const DRIVE_FIXED: u32 = 3;
const NETWORK_RELATIVE_LINK_HEADER_SIZE: u32 = 0x14;

const ENVIRONMENT_VARIABLE_SIGNATURE: u32 = 0xA000_0001;
const ENVIRONMENT_VARIABLE_SIZE: u32 = 0x314;
const TRACKER_SIGNATURE: u32 = 0xA000_0003;
const SPECIAL_FOLDER_SIGNATURE: u32 = 0xA000_0005;
const KNOWN_FOLDER_SIGNATURE: u32 = 0xA000_000B;
const MAX_PATH: usize = 260;

#[derive(Debug)]
pub enum LnkError {
    Truncated,
    NotShortcut,
    StringTooLong,
    TargetTooLong,
}

impl fmt::Display for LnkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LnkError::Truncated => "the shortcut ends early",
            LnkError::NotShortcut => "not a Windows shortcut",
            LnkError::StringTooLong => "a string is longer than 65535 characters",
            LnkError::TargetTooLong => "the target is longer than 259 characters",
        })
    }
}

impl std::error::Error for LnkError {}

type Result<T> = std::result::Result<T, LnkError>;

/// A shortcut, kept byte-exact where it is not edited.
pub struct ShellLink {
    header: [u8; HEADER_SIZE as usize],
    /// The LinkTargetIDList without its size prefix.
    id_list: Option<Vec<u8>>,
    link_info: Option<LinkInfo>,
    name: Option<String>,
    relative_path: Option<String>,
    working_dir: Option<String>,
    arguments: Option<String>,
    icon_location: Option<String>,
    extra: Vec<ExtraBlock>,
}

struct LinkInfo {
    raw: Vec<u8>,
    path: Option<String>,
}

struct ExtraBlock {
    signature: u32,
    /// The block after its size and signature.
    data: Vec<u8>,
}

impl ShellLink {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut reader = Reader { data, pos: 0 };
        let header: [u8; HEADER_SIZE as usize] = reader
            .bytes(HEADER_SIZE as usize)?
            .try_into()
            .expect("the header length");
        if header[..4] != HEADER_SIZE.to_le_bytes() || header[4..20] != CLSID {
            return Err(LnkError::NotShortcut);
        }
        let flags = u32::from_le_bytes(header[FLAGS_OFFSET..FLAGS_OFFSET + 4].try_into().unwrap());
        let unicode = flags & IS_UNICODE != 0;

        let id_list = if flags & HAS_LINK_TARGET_ID_LIST != 0 {
            let size = reader.u16()? as usize;
            Some(reader.bytes(size)?.to_vec())
        } else {
            None
        };
        let link_info = if flags & HAS_LINK_INFO != 0 {
            let size = reader.peek_u32()? as usize;
            let raw = reader.bytes(size)?.to_vec();
            let path = link_info_path(&raw)?;
            Some(LinkInfo { raw, path })
        } else {
            None
        };
        let mut string = |flag: u32| -> Result<Option<String>> {
            if flags & flag == 0 {
                return Ok(None);
            }
            let count = reader.u16()? as usize;
            Ok(Some(if unicode {
                let bytes = reader.bytes(count * 2)?;
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| u16::from_le_bytes(pair))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                ansi(reader.bytes(count)?)
            }))
        };
        let name = string(HAS_NAME)?;
        let relative_path = string(HAS_RELATIVE_PATH)?;
        let working_dir = string(HAS_WORKING_DIR)?;
        let arguments = string(HAS_ARGUMENTS)?;
        let icon_location = string(HAS_ICON_LOCATION)?;

        let mut extra = Vec::new();
        while reader.remaining() >= 4 {
            let size = reader.u32()? as usize;
            if size < 8 {
                break;
            }
            let signature = reader.u32()?;
            let data = reader.bytes(size - 8)?.to_vec();
            extra.push(ExtraBlock { signature, data });
        }
        Ok(ShellLink {
            header,
            id_list,
            link_info,
            name,
            relative_path,
            working_dir,
            arguments,
            icon_location,
            extra,
        })
    }

    fn flags(&self) -> u32 {
        u32::from_le_bytes(
            self.header[FLAGS_OFFSET..FLAGS_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    }

    fn set_flag(&mut self, flag: u32, on: bool) {
        let flags = if on {
            self.flags() | flag
        } else {
            self.flags() & !flag
        };
        self.header[FLAGS_OFFSET..FLAGS_OFFSET + 4].copy_from_slice(&flags.to_le_bytes());
    }

    /// The target as stored: the LinkInfo path, or else the unexpanded path of the
    /// EnvironmentVariableDataBlock.
    pub fn target(&self) -> Option<String> {
        if let Some(path) = self
            .link_info
            .as_ref()
            .and_then(|info| info.path.as_deref())
        {
            return Some(path.to_owned());
        }
        self.environment_block().and_then(environment_target)
    }

    pub fn arguments(&self) -> Option<&str> {
        self.arguments.as_deref().map(trim_nul)
    }

    pub fn working_dir(&self) -> Option<&str> {
        self.working_dir.as_deref().map(trim_nul)
    }

    /// Points the shortcut at `path`. A path holding `%` is stored unexpanded in an
    /// EnvironmentVariableDataBlock, a `\\server\share` path as a network link, and anything else
    /// as a local path. The item ID list, relative path, and the blocks that describe the old
    /// target are dropped, since they would still name it.
    pub fn set_target(&mut self, path: &str) -> Result<()> {
        self.id_list = None;
        self.set_flag(HAS_LINK_TARGET_ID_LIST, false);
        self.relative_path = None;
        self.set_flag(HAS_RELATIVE_PATH, false);
        self.extra.retain(|block| {
            ![
                TRACKER_SIGNATURE,
                SPECIAL_FOLDER_SIGNATURE,
                KNOWN_FOLDER_SIGNATURE,
            ]
            .contains(&block.signature)
        });
        if path.contains('%') {
            self.link_info = None;
            self.set_flag(HAS_LINK_INFO, false);
            self.extra
                .retain(|block| block.signature != ENVIRONMENT_VARIABLE_SIGNATURE);
            self.extra.push(ExtraBlock {
                signature: ENVIRONMENT_VARIABLE_SIGNATURE,
                data: environment_data(path)?,
            });
            self.set_flag(HAS_EXP_STRING, true);
        } else {
            let raw = build_link_info(path);
            self.link_info = Some(LinkInfo {
                raw,
                path: Some(path.to_owned()),
            });
            self.set_flag(HAS_LINK_INFO, true);
            self.extra
                .retain(|block| block.signature != ENVIRONMENT_VARIABLE_SIGNATURE);
            self.set_flag(HAS_EXP_STRING, false);
        }
        Ok(())
    }

    /// Sets the arguments; empty removes them.
    pub fn set_arguments(&mut self, arguments: &str) {
        self.arguments = Some(arguments.to_owned()).filter(|s| !s.is_empty());
        self.set_flag(HAS_ARGUMENTS, self.arguments.is_some());
    }

    /// Sets the working directory; empty removes it.
    pub fn set_working_dir(&mut self, dir: &str) {
        self.working_dir = Some(dir.to_owned()).filter(|s| !s.is_empty());
        self.set_flag(HAS_WORKING_DIR, self.working_dir.is_some());
    }

    fn environment_block(&self) -> Option<&ExtraBlock> {
        self.extra
            .iter()
            .find(|block| block.signature == ENVIRONMENT_VARIABLE_SIGNATURE)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let strings = [
            &self.name,
            &self.relative_path,
            &self.working_dir,
            &self.arguments,
            &self.icon_location,
        ];
        // An ANSI shortcut stays ANSI unless an edit needs more than Latin-1.
        let unicode = self.flags() & IS_UNICODE != 0
            || strings
                .iter()
                .copied()
                .flatten()
                .any(|s| !s.chars().all(|c| (c as u32) < 0x100));
        let mut header = self.header;
        if unicode {
            let flags = self.flags() | IS_UNICODE;
            header[FLAGS_OFFSET..FLAGS_OFFSET + 4].copy_from_slice(&flags.to_le_bytes());
        }
        let mut out = header.to_vec();
        if let Some(id_list) = &self.id_list {
            let size = u16::try_from(id_list.len()).map_err(|_| LnkError::StringTooLong)?;
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(id_list);
        }
        if let Some(info) = &self.link_info {
            out.extend_from_slice(&info.raw);
        }
        for string in strings.into_iter().flatten() {
            if unicode {
                let units: Vec<u16> = string.encode_utf16().collect();
                let count = u16::try_from(units.len()).map_err(|_| LnkError::StringTooLong)?;
                out.extend_from_slice(&count.to_le_bytes());
                for unit in units {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
            } else {
                let bytes = to_ansi(string);
                let count = u16::try_from(bytes.len()).map_err(|_| LnkError::StringTooLong)?;
                out.extend_from_slice(&count.to_le_bytes());
                out.extend_from_slice(&bytes);
            }
        }
        for block in &self.extra {
            let size = u32::try_from(block.data.len() + 8).map_err(|_| LnkError::StringTooLong)?;
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&block.signature.to_le_bytes());
            out.extend_from_slice(&block.data);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        Ok(out)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(count).ok_or(LnkError::Truncated)?;
        let bytes = self.data.get(self.pos..end).ok_or(LnkError::Truncated)?;
        self.pos = end;
        Ok(bytes)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn peek_u32(&self) -> Result<u32> {
        Reader {
            data: self.data,
            pos: self.pos,
        }
        .u32()
    }
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Reader { data, pos: offset }.u32()
}

/// Wine counts a string's terminator among its characters; drop it.
fn trim_nul(text: &str) -> &str {
    text.trim_end_matches('\0')
}

/// Bytes in the system code page, read as Latin-1 so they survive a round trip.
fn ansi(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn to_ansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(c as u32).unwrap_or(b'?'))
        .collect()
}

fn ansi_z(data: &[u8], offset: usize) -> Result<String> {
    let rest = data.get(offset..).ok_or(LnkError::Truncated)?;
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(LnkError::Truncated)?;
    Ok(ansi(&rest[..end]))
}

fn utf16_z(data: &[u8], offset: usize) -> Result<String> {
    let rest = data.get(offset..).ok_or(LnkError::Truncated)?;
    let mut units = Vec::new();
    for &pair in rest.as_chunks::<2>().0 {
        let unit = u16::from_le_bytes(pair);
        if unit == 0 {
            return Ok(String::from_utf16_lossy(&units));
        }
        units.push(unit);
    }
    Err(LnkError::Truncated)
}

fn utf16_z_bytes(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn ansi_z_bytes(text: &str) -> Vec<u8> {
    let mut bytes = to_ansi(text);
    bytes.push(0);
    bytes
}

/// The path a LinkInfo structure names: the local base path and common suffix, or the network
/// share name and suffix. The Unicode copies win over the ANSI ones.
fn link_info_path(info: &[u8]) -> Result<Option<String>> {
    let header_size = u32_at(info, 4)? as usize;
    let flags = u32_at(info, 8)?;
    let unicode = header_size >= LINK_INFO_HEADER_SIZE_UNICODE as usize;
    let suffix_offset = u32_at(info, 0x18)? as usize;
    let suffix = match u32_at(info, 0x20).ok().filter(|&o| unicode && o != 0) {
        Some(offset) => utf16_z(info, offset as usize)?,
        None => ansi_z(info, suffix_offset)?,
    };
    if flags & VOLUME_ID_AND_LOCAL_BASE_PATH != 0 {
        let base_offset = u32_at(info, 0x10)? as usize;
        let base = match u32_at(info, 0x1C).ok().filter(|&o| unicode && o != 0) {
            Some(offset) => utf16_z(info, offset as usize)?,
            None => ansi_z(info, base_offset)?,
        };
        return Ok(Some(format!("{base}{suffix}")));
    }
    if flags & COMMON_NETWORK_RELATIVE_LINK_AND_PATH_SUFFIX != 0 {
        let link_offset = u32_at(info, 0x14)? as usize;
        let link = info.get(link_offset..).ok_or(LnkError::Truncated)?;
        let name_offset = u32_at(link, 8)? as usize;
        let name = if name_offset > NETWORK_RELATIVE_LINK_HEADER_SIZE as usize {
            utf16_z(link, u32_at(link, 0x14)? as usize)?
        } else {
            ansi_z(link, name_offset)?
        };
        return Ok(Some(if suffix.is_empty() {
            name
        } else {
            format!("{name}\\{suffix}")
        }));
    }
    Ok(None)
}

/// A LinkInfo naming `path`, with Unicode copies of its strings.
fn build_link_info(path: &str) -> Vec<u8> {
    let network = path.starts_with("\\\\");
    let (base, suffix) = if network {
        // \\server\share is the share; what follows is the suffix.
        let trimmed = &path[2..];
        match trimmed.match_indices('\\').nth(1) {
            Some((at, _)) => (&path[..2 + at], &trimmed[at + 1..]),
            None => (path, ""),
        }
    } else {
        (path, "")
    };
    let header_size = LINK_INFO_HEADER_SIZE_UNICODE;
    let mut body = Vec::new();
    let mut volume_offset = 0;
    let mut base_offset = 0;
    let mut link_offset = 0;
    let mut base_unicode_offset = 0;
    if network {
        link_offset = header_size + body.len() as u32;
        let name = ansi_z_bytes(base);
        let link_size = NETWORK_RELATIVE_LINK_HEADER_SIZE + name.len() as u32;
        body.extend_from_slice(&link_size.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes()); // flags: no device, no provider type
        body.extend_from_slice(&NETWORK_RELATIVE_LINK_HEADER_SIZE.to_le_bytes()); // NetNameOffset
        body.extend_from_slice(&0u32.to_le_bytes()); // DeviceNameOffset
        body.extend_from_slice(&0u32.to_le_bytes()); // NetworkProviderType
        body.extend_from_slice(&name);
    } else {
        volume_offset = header_size + body.len() as u32;
        // A fixed drive with no serial number and an empty label.
        body.extend_from_slice(&0x11u32.to_le_bytes());
        body.extend_from_slice(&DRIVE_FIXED.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&0x10u32.to_le_bytes());
        body.push(0);
        base_offset = header_size + body.len() as u32;
        body.extend_from_slice(&ansi_z_bytes(base));
    }
    let suffix_offset = header_size + body.len() as u32;
    body.extend_from_slice(&ansi_z_bytes(suffix));
    if !network {
        base_unicode_offset = header_size + body.len() as u32;
        body.extend_from_slice(&utf16_z_bytes(base));
    }
    let suffix_unicode_offset = header_size + body.len() as u32;
    body.extend_from_slice(&utf16_z_bytes(suffix));

    let flags = if network {
        COMMON_NETWORK_RELATIVE_LINK_AND_PATH_SUFFIX
    } else {
        VOLUME_ID_AND_LOCAL_BASE_PATH
    };
    let mut info = Vec::with_capacity(header_size as usize + body.len());
    for word in [
        header_size + body.len() as u32,
        header_size,
        flags,
        volume_offset,
        base_offset,
        link_offset,
        suffix_offset,
        base_unicode_offset,
        suffix_unicode_offset,
    ] {
        info.extend_from_slice(&word.to_le_bytes());
    }
    info.extend_from_slice(&body);
    info
}

/// The unexpanded target an EnvironmentVariableDataBlock holds, Unicode winning over ANSI.
fn environment_target(block: &ExtraBlock) -> Option<String> {
    let unicode = utf16_z(&block.data, MAX_PATH)
        .ok()
        .filter(|s| !s.is_empty());
    unicode.or_else(|| ansi_z(&block.data, 0).ok().filter(|s| !s.is_empty()))
}

fn environment_data(path: &str) -> Result<Vec<u8>> {
    let mut data = vec![0u8; ENVIRONMENT_VARIABLE_SIZE as usize - 8];
    let ansi = to_ansi(path);
    if ansi.len() >= MAX_PATH || path.encode_utf16().count() >= MAX_PATH {
        return Err(LnkError::TargetTooLong);
    }
    data[..ansi.len()].copy_from_slice(&ansi);
    for (i, unit) in path.encode_utf16().enumerate() {
        data[MAX_PATH + i * 2..MAX_PATH + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN98: &[u8] = include_bytes!("testdata/win98_ansi.lnk");
    const WIN7: &[u8] = include_bytes!("testdata/win7_unicode.lnk");
    const WINE: &[u8] = include_bytes!("testdata/wine.lnk");

    #[test]
    fn round_trips_unchanged() {
        for data in [WIN98, WIN7, WINE] {
            assert_eq!(ShellLink::parse(data).unwrap().to_bytes().unwrap(), data);
        }
    }

    #[test]
    fn reads_fields() {
        let link = ShellLink::parse(WIN98).unwrap();
        assert_eq!(
            link.target(),
            Some("D:\\tools\\3DMark\\3DMark99 Max\\3dmark.exe".into())
        );
        assert_eq!(link.arguments(), None);
        assert_eq!(link.working_dir(), Some("D:\\tools\\3DMark\\3DMark99 Max"));

        let link = ShellLink::parse(WIN7).unwrap();
        assert_eq!(
            link.target(),
            Some("D:\\KF2_Main\\Binaries\\Win64\\KFGame.exe".into())
        );
        assert_eq!(link.arguments(), Some("editor"));
        assert_eq!(link.working_dir(), Some("D:\\KF2_Main\\Binaries\\Win64"));

        let link = ShellLink::parse(WINE).unwrap();
        assert_eq!(
            link.target(),
            Some("C:\\Program Files (x86)\\Tibo Software\\BadToys 3D\\bt3d.exe".into())
        );
        assert_eq!(
            link.working_dir(),
            Some("C:\\Program Files (x86)\\Tibo Software\\BadToys 3D")
        );
    }

    #[test]
    fn edits_survive_a_round_trip() {
        let mut link = ShellLink::parse(WIN7).unwrap();
        link.set_target("C:\\Games\\KF2\\KFGame.exe").unwrap();
        link.set_arguments("-windowed");
        link.set_working_dir("C:\\Games\\KF2");
        let link = ShellLink::parse(&link.to_bytes().unwrap()).unwrap();
        assert_eq!(link.target(), Some("C:\\Games\\KF2\\KFGame.exe".into()));
        assert_eq!(link.arguments(), Some("-windowed"));
        assert_eq!(link.working_dir(), Some("C:\\Games\\KF2"));
        assert!(link.id_list.is_none());
        assert!(link.relative_path.is_none());
        assert!(link.extra.iter().all(|b| b.signature != TRACKER_SIGNATURE));
    }

    #[test]
    fn network_and_environment_targets() {
        let mut link = ShellLink::parse(WIN98).unwrap();
        link.set_target("\\\\server\\share\\dir\\app.exe").unwrap();
        let link = ShellLink::parse(&link.to_bytes().unwrap()).unwrap();
        assert_eq!(
            link.target(),
            Some("\\\\server\\share\\dir\\app.exe".into())
        );

        let mut link = ShellLink::parse(WIN98).unwrap();
        link.set_target("%ProgramFiles%\\app.exe").unwrap();
        let data = link.to_bytes().unwrap();
        let link = ShellLink::parse(&data).unwrap();
        assert_eq!(link.target(), Some("%ProgramFiles%\\app.exe".into()));
        assert!(link.link_info.is_none());
    }

    #[test]
    fn ansi_goes_unicode_when_needed() {
        let mut link = ShellLink::parse(WIN98).unwrap();
        link.set_arguments("日本");
        let data = link.to_bytes().unwrap();
        let link = ShellLink::parse(&data).unwrap();
        assert!(link.flags() & IS_UNICODE != 0);
        assert_eq!(link.arguments(), Some("日本"));
        assert_eq!(link.working_dir(), Some("D:\\tools\\3DMark\\3DMark99 Max"));
    }

    #[test]
    fn rejects_other_files() {
        assert!(matches!(
            ShellLink::parse(b"MZ not a shortcut").err(),
            Some(LnkError::Truncated | LnkError::NotShortcut)
        ));
    }
}
