use super::Sector;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Filesystem {
    Fat12,
    Fat16,
    Fat32,
    ExFat,
    Unknown,
}

impl Filesystem {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fat12 => "fat12",
            Self::Fat16 => "fat16",
            Self::Fat32 => "fat32",
            Self::ExFat => "exfat",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Partition {
    pub bootable: bool,
    pub type_code: u8,
    pub first_lba: u32,
    pub sector_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskLayout {
    MasterBootRecord {
        partitions: [Option<Partition>; 4],
        partition_count: u8,
    },
    SuperFloppy(Filesystem),
    Unknown,
}

impl DiskLayout {
    pub const fn name(self) -> &'static str {
        match self {
            Self::MasterBootRecord { .. } => "mbr",
            Self::SuperFloppy(_) => "superfloppy",
            Self::Unknown => "unknown",
        }
    }

    pub const fn first_partition(self) -> Option<Partition> {
        match self {
            Self::MasterBootRecord { partitions, .. } => {
                let [first, second, third, fourth] = partitions;
                match first {
                    Some(partition) => Some(partition),
                    None => match second {
                        Some(partition) => Some(partition),
                        None => match third {
                            Some(partition) => Some(partition),
                            None => fourth,
                        },
                    },
                }
            }
            Self::SuperFloppy(_) | Self::Unknown => None,
        }
    }
}

pub fn sector_fingerprint(sector: &Sector) -> u32 {
    let mut fingerprint = 0x811C_9DC5_u32;
    for byte in sector.as_bytes() {
        fingerprint ^= u32::from(*byte);
        fingerprint = fingerprint.wrapping_mul(0x0100_0193);
    }
    fingerprint
}

pub fn inspect_sector_zero(sector: &Sector) -> DiskLayout {
    let bytes = sector.as_bytes();
    if bytes[510..512] != [0x55, 0xAA] {
        return DiskLayout::Unknown;
    }

    let mut partitions = [None; 4];
    let mut partition_count = 0;
    for (index, slot) in partitions.iter_mut().enumerate() {
        let offset = 446 + index * 16;
        let type_code = bytes[offset + 4];
        let first_lba = u32::from_le_bytes([
            bytes[offset + 8],
            bytes[offset + 9],
            bytes[offset + 10],
            bytes[offset + 11],
        ]);
        let sector_count = u32::from_le_bytes([
            bytes[offset + 12],
            bytes[offset + 13],
            bytes[offset + 14],
            bytes[offset + 15],
        ]);
        if type_code != 0 && sector_count != 0 {
            *slot = Some(Partition {
                bootable: bytes[offset] == 0x80,
                type_code,
                first_lba,
                sector_count,
            });
            partition_count += 1;
        }
    }

    if partition_count > 0 {
        return DiskLayout::MasterBootRecord {
            partitions,
            partition_count,
        };
    }

    let filesystem = inspect_filesystem(sector);
    if filesystem == Filesystem::Unknown {
        DiskLayout::Unknown
    } else {
        DiskLayout::SuperFloppy(filesystem)
    }
}

pub fn inspect_filesystem(sector: &Sector) -> Filesystem {
    let bytes = sector.as_bytes();
    if bytes.get(3..11) == Some(b"EXFAT   ") {
        return Filesystem::ExFat;
    }
    if bytes.get(82..90) == Some(b"FAT32   ") {
        return Filesystem::Fat32;
    }
    match bytes.get(54..62) {
        Some(b"FAT12   ") => Filesystem::Fat12,
        Some(b"FAT16   ") => Filesystem::Fat16,
        _ => Filesystem::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiskLayout, Filesystem, Partition, inspect_filesystem, inspect_sector_zero,
        sector_fingerprint,
    };
    use crate::storage::Sector;

    #[test]
    fn sector_fingerprint_changes_when_sector_contents_change() {
        let mut sector = Sector::zeroed();
        let before = sector_fingerprint(&sector);
        sector.as_bytes_mut()[511] = 1;
        assert_ne!(sector_fingerprint(&sector), before);
    }

    #[test]
    fn parses_a_partitioned_card_without_reading_past_sector_zero() {
        let mut sector = Sector::zeroed();
        let bytes = sector.as_bytes_mut();
        bytes[446] = 0x80;
        bytes[450] = 0x0C;
        bytes[454..458].copy_from_slice(&2_048_u32.to_le_bytes());
        bytes[458..462].copy_from_slice(&31_000_000_u32.to_le_bytes());
        bytes[510..512].copy_from_slice(&[0x55, 0xAA]);

        assert_eq!(
            inspect_sector_zero(&sector),
            DiskLayout::MasterBootRecord {
                partitions: [
                    Some(Partition {
                        bootable: true,
                        type_code: 0x0C,
                        first_lba: 2_048,
                        sector_count: 31_000_000,
                    }),
                    None,
                    None,
                    None,
                ],
                partition_count: 1,
            }
        );
    }

    #[test]
    fn distinguishes_fat_and_exfat_boot_sectors() {
        for (offset, label, expected) in [
            (54, b"FAT16   ".as_slice(), Filesystem::Fat16),
            (82, b"FAT32   ".as_slice(), Filesystem::Fat32),
            (3, b"EXFAT   ".as_slice(), Filesystem::ExFat),
        ] {
            let mut sector = Sector::zeroed();
            sector.as_bytes_mut()[offset..offset + label.len()].copy_from_slice(label);
            assert_eq!(inspect_filesystem(&sector), expected);
        }
    }

    #[test]
    fn recognizes_a_superfloppy_only_with_a_boot_signature() {
        let mut sector = Sector::zeroed();
        sector.as_bytes_mut()[82..90].copy_from_slice(b"FAT32   ");
        assert_eq!(inspect_sector_zero(&sector), DiskLayout::Unknown);

        sector.as_bytes_mut()[510..512].copy_from_slice(&[0x55, 0xAA]);
        assert_eq!(
            inspect_sector_zero(&sector),
            DiskLayout::SuperFloppy(Filesystem::Fat32)
        );
    }
}
