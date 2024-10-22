use crate::{Key, Key1, Key2, Key4};

/// We DO NOT impl Serialize/Deserialize for this type because we use
/// non-postcard-compatible format (externally tagged)
#[derive(Debug, Copy, Clone)]
pub enum VarKey {
    Key1(Key1),
    Key2(Key2),
    Key4(Key4),
    Key8(Key),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VarKeyKind {
    Key1,
    Key2,
    Key4,
    Key8,
}

impl VarKey {
    pub fn shrink_to(&mut self, kind: VarKeyKind) {
        match (&self, kind) {
            (VarKey::Key1(_), _) => {
                // Nothing to shrink
            },
            (VarKey::Key2(key2), VarKeyKind::Key1) => {
                *self = VarKey::Key1(Key1::from_key2(*key2));
            },
            (VarKey::Key2(_), _) => {
                // We are already as small or smaller than the request
            },
            (VarKey::Key4(key4), VarKeyKind::Key1) => {
                *self = VarKey::Key1(Key1::from_key4(*key4));
            },
            (VarKey::Key4(key4), VarKeyKind::Key2) => {
                *self = VarKey::Key2(Key2::from_key4(*key4));
            },
            (VarKey::Key4(_), _) => {
                // We are already as small or smaller than the request
            },
            (VarKey::Key8(key), VarKeyKind::Key1) => {
                *self = VarKey::Key1(Key1::from_key8(*key));
            },
            (VarKey::Key8(key), VarKeyKind::Key2) => {
                *self = VarKey::Key2(Key2::from_key8(*key));
            },
            (VarKey::Key8(key), VarKeyKind::Key4) => {
                *self = VarKey::Key4(Key4::from_key8(*key));
            },
            (VarKey::Key8(_), _) => {
                // Nothing to do
            },
        }
    }

    pub fn kind(&self) -> VarKeyKind {
        match self {
            VarKey::Key1(_) => VarKeyKind::Key1,
            VarKey::Key2(_) => VarKeyKind::Key2,
            VarKey::Key4(_) => VarKeyKind::Key4,
            VarKey::Key8(_) => VarKeyKind::Key8,
        }
    }
}

/// We implement PartialEq MANUALLY for VarKey, because keys of different lengths SHOULD compare
/// as equal.
impl PartialEq for VarKey {
    fn eq(&self, other: &Self) -> bool {
        // figure out the minimum length
        match (self, other) {
            // Matching kinds
            (VarKey::Key1(self_key), VarKey::Key1(other_key)) => self_key.0.eq(&other_key.0),
            (VarKey::Key2(self_key), VarKey::Key2(other_key)) => self_key.0.eq(&other_key.0),
            (VarKey::Key4(self_key), VarKey::Key4(other_key)) => self_key.0.eq(&other_key.0),
            (VarKey::Key8(self_key), VarKey::Key8(other_key)) => self_key.0.eq(&other_key.0),

            // For the rest of the options, degrade the LARGER key to the SMALLER key, and then
            // check for equivalence after that.
            (VarKey::Key1(this), VarKey::Key2(other)) => {
                let other = Key1::from_key2(*other);
                this.0.eq(&other.0)
            }
            (VarKey::Key1(this), VarKey::Key4(other)) => {
                let other = Key1::from_key4(*other);
                this.0.eq(&other.0)
            }
            (VarKey::Key1(this), VarKey::Key8(other)) => {
                let other = Key1::from_key8(*other);
                this.0.eq(&other.0)
            }
            (VarKey::Key2(this), VarKey::Key1(other)) => {
                let this = Key1::from_key2(*this);
                this.0.eq(&other.0)
            }
            (VarKey::Key2(this), VarKey::Key4(other)) => {
                let other = Key2::from_key4(*other);
                this.0.eq(&other.0)
            }
            (VarKey::Key2(this), VarKey::Key8(other)) => {
                let other = Key2::from_key8(*other);
                this.0.eq(&other.0)
            }
            (VarKey::Key4(this), VarKey::Key1(other)) => {
                let this = Key1::from_key4(*this);
                this.0.eq(&other.0)
            }
            (VarKey::Key4(this), VarKey::Key2(other)) => {
                let this = Key2::from_key4(*this);
                this.0.eq(&other.0)
            }
            (VarKey::Key4(this), VarKey::Key8(other)) => {
                let other = Key4::from_key8(*other);
                this.0.eq(&other.0)
            }
            (VarKey::Key8(this), VarKey::Key1(other)) => {
                let this = Key1::from_key8(*this);
                this.0.eq(&other.0)
            }
            (VarKey::Key8(this), VarKey::Key2(other)) => {
                let this = Key2::from_key8(*this);
                this.0.eq(&other.0)
            }
            (VarKey::Key8(this), VarKey::Key4(other)) => {
                let this = Key4::from_key8(*this);
                this.0.eq(&other.0)
            }
        }
    }
}

/// NOTE: We use the standard PartialEq here, as we DO NOT treat sequence
/// numbers of different lengths as equivalent.
///
/// We DO NOT impl Serialize/Deserialize for this type because we use
/// non-postcard-compatible format (externally tagged)
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum VarSeq {
    Seq1(u8),
    Seq2(u16),
    Seq4(u32),
}

impl From<u8> for VarSeq {
    fn from(value: u8) -> Self {
        Self::Seq1(value)
    }
}

impl From<u16> for VarSeq {
    fn from(value: u16) -> Self {
        Self::Seq2(value)
    }
}

impl From<u32> for VarSeq {
    fn from(value: u32) -> Self {
        Self::Seq4(value)
    }
}

impl VarSeq {
    pub fn resize(&mut self, kind: VarSeqKind) {
        match (&self, kind) {
            (VarSeq::Seq1(_), VarSeqKind::Seq1) => {},
            (VarSeq::Seq2(_), VarSeqKind::Seq2) => {},
            (VarSeq::Seq4(_), VarSeqKind::Seq4) => {},
            (VarSeq::Seq1(s), VarSeqKind::Seq2) => {
                *self = VarSeq::Seq2((*s).into());
            },
            (VarSeq::Seq1(s), VarSeqKind::Seq4) => {
                *self = VarSeq::Seq4((*s).into());
            },
            (VarSeq::Seq2(s), VarSeqKind::Seq1) => {
                *self = VarSeq::Seq1((*s) as u8);
            },
            (VarSeq::Seq2(s), VarSeqKind::Seq4) => {
                *self = VarSeq::Seq4((*s).into());
            },
            (VarSeq::Seq4(s), VarSeqKind::Seq1) => {
                *self = VarSeq::Seq1((*s) as u8);
            },
            (VarSeq::Seq4(s), VarSeqKind::Seq2) => {
                *self = VarSeq::Seq2((*s) as u16);
            },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VarSeqKind {
    Seq1,
    Seq2,
    Seq4,
}

/// NOTE: We use the standard PartialEq here as it will do the correct things.
///
/// Sequence numbers must be EXACTLY the same, and keys must be equivalent when
/// degraded to the smaller of the two.
///
/// We DO NOT impl Serialize/Deserialize for this type because we use
/// non-postcard-compatible format (externally tagged)
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct VarHeader {
    pub key: VarKey,
    pub seq_no: VarSeq,
}

#[allow(clippy::unusual_byte_groupings)]
impl VarHeader {
    pub const KEY_ONE_BITS: u8 = 0b00_00_0000;
    pub const KEY_TWO_BITS: u8 = 0b01_00_0000;
    pub const KEY_FOUR_BITS: u8 = 0b10_00_0000;
    pub const KEY_EIGHT_BITS: u8 = 0b11_00_0000;
    pub const KEY_MASK_BITS: u8 = 0b11_00_0000;

    pub const SEQ_ONE_BITS: u8 = 0b00_00_0000;
    pub const SEQ_TWO_BITS: u8 = 0b00_01_0000;
    pub const SEQ_FOUR_BITS: u8 = 0b00_10_0000;
    pub const SEQ_MASK_BITS: u8 = 0b00_11_0000;

    pub const VER_ZERO_BITS: u8 = 0b00_00_0000;
    pub const VER_MASK_BITS: u8 = 0b00_00_1111;

    #[cfg(feature = "use-std")]
    pub fn write_to_vec(&self) -> Vec<u8> {
        // start with placeholder byte
        let mut out = vec![0u8; 1];
        let mut disc_out: u8;
        match &self.key {
            VarKey::Key1(k) => {
                disc_out = Self::KEY_ONE_BITS;
                out.push(k.0);
            }
            VarKey::Key2(k) => {
                disc_out = Self::KEY_TWO_BITS;
                out.extend_from_slice(&k.0);
            }
            VarKey::Key4(k) => {
                disc_out = Self::KEY_FOUR_BITS;
                out.extend_from_slice(&k.0);
            }
            VarKey::Key8(k) => {
                disc_out = Self::KEY_EIGHT_BITS;
                out.extend_from_slice(&k.0);
            }
        }
        match &self.seq_no {
            VarSeq::Seq1(s) => {
                disc_out |= Self::SEQ_ONE_BITS;
                out.push(*s);
            }
            VarSeq::Seq2(s) => {
                disc_out |= Self::SEQ_TWO_BITS;
                out.extend_from_slice(&s.to_le_bytes());
            }
            VarSeq::Seq4(s) => {
                disc_out |= Self::SEQ_FOUR_BITS;
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
        // push discriminant to the end...
        out.push(disc_out);
        // ...and swap-remove the placeholder byte, moving the discriminant to the front
        out.swap_remove(0);
        out
    }

    pub fn write_to_slice<'a>(&self, buf: &'a mut [u8]) -> Option<(&'a mut [u8], &'a mut [u8])> {
        let (disc_out, mut remain) = buf.split_first_mut()?;
        let mut used = 1;

        match &self.key {
            VarKey::Key1(k) => {
                *disc_out = Self::KEY_ONE_BITS;
                let (keybs, remain2) = remain.split_first_mut()?;
                *keybs = k.0;
                remain = remain2;
                used += 1;
            }
            VarKey::Key2(k) => {
                *disc_out = Self::KEY_TWO_BITS;
                let (keybs, remain2) = remain.split_at_mut_checked(2)?;
                keybs.copy_from_slice(&k.0);
                remain = remain2;
                used += 2;
            }
            VarKey::Key4(k) => {
                *disc_out = Self::KEY_FOUR_BITS;
                let (keybs, remain2) = remain.split_at_mut_checked(4)?;
                keybs.copy_from_slice(&k.0);
                remain = remain2;
                used += 4;
            }
            VarKey::Key8(k) => {
                *disc_out = Self::KEY_EIGHT_BITS;
                let (keybs, remain2) = remain.split_at_mut_checked(8)?;
                keybs.copy_from_slice(&k.0);
                remain = remain2;
                used += 8;
            }
        }
        match &self.seq_no {
            VarSeq::Seq1(s) => {
                *disc_out |= Self::SEQ_ONE_BITS;
                let (seqbs, _) = remain.split_first_mut()?;
                *seqbs = *s;
                used += 1;
            }
            VarSeq::Seq2(s) => {
                *disc_out |= Self::SEQ_TWO_BITS;
                let (seqbs, _) = remain.split_at_mut_checked(2)?;
                seqbs.copy_from_slice(&s.to_le_bytes());
                used += 2;
            }
            VarSeq::Seq4(s) => {
                *disc_out |= Self::SEQ_FOUR_BITS;
                let (seqbs, _) = remain.split_at_mut_checked(4)?;
                seqbs.copy_from_slice(&s.to_le_bytes());
                used += 4;
            }
        }
        Some(buf.split_at_mut(used))
    }

    pub fn take_from_slice(buf: &[u8]) -> Option<(Self, &[u8])> {
        let (disc, mut remain) = buf.split_first()?;

        // For now, we only trust version zero
        if (*disc & Self::VER_MASK_BITS) != Self::VER_ZERO_BITS {
            return None;
        }

        let key = match (*disc) & Self::KEY_MASK_BITS {
            Self::KEY_ONE_BITS => {
                let (keybs, remain2) = remain.split_first()?;
                remain = remain2;
                VarKey::Key1(Key1(*keybs))
            }
            Self::KEY_TWO_BITS => {
                let (keybs, remain2) = remain.split_at_checked(2)?;
                remain = remain2;
                let mut buf = [0u8; 2];
                buf.copy_from_slice(keybs);
                VarKey::Key2(Key2(buf))
            }
            Self::KEY_FOUR_BITS => {
                let (keybs, remain2) = remain.split_at_checked(4)?;
                remain = remain2;
                let mut buf = [0u8; 4];
                buf.copy_from_slice(keybs);
                VarKey::Key4(Key4(buf))
            }
            Self::KEY_EIGHT_BITS => {
                let (keybs, remain2) = remain.split_at_checked(8)?;
                remain = remain2;
                let mut buf = [0u8; 8];
                buf.copy_from_slice(keybs);
                VarKey::Key8(Key(buf))
            }
            // Impossible: all bits covered
            _ => unreachable!(),
        };
        let seq_no = match (*disc) & Self::SEQ_MASK_BITS {
            Self::SEQ_ONE_BITS => {
                let (seqbs, remain3) = remain.split_first()?;
                remain = remain3;
                VarSeq::Seq1(*seqbs)
            }
            Self::SEQ_TWO_BITS => {
                let (seqbs, remain3) = remain.split_at_checked(2)?;
                remain = remain3;
                let mut buf = [0u8; 2];
                buf.copy_from_slice(seqbs);
                VarSeq::Seq2(u16::from_le_bytes(buf))
            }
            Self::SEQ_FOUR_BITS => {
                let (seqbs, remain3) = remain.split_at_checked(4)?;
                remain = remain3;
                let mut buf = [0u8; 4];
                buf.copy_from_slice(seqbs);
                VarSeq::Seq4(u32::from_le_bytes(buf))
            }
            // Possible (could be 0b11), is invalid
            _ => return None,
        };
        Some((Self { key, seq_no }, remain))
    }
}

#[cfg(test)]
mod test {
    use super::{VarHeader, VarKey, VarSeq};
    use crate::{Key, Key1, Key2};

    #[test]
    fn wire_format() {
        let checks: &[(_, &[u8])] = &[
            (
                VarHeader {
                    key: VarKey::Key1(Key1(0)),
                    seq_no: VarSeq::Seq1(0x00),
                },
                &[
                    VarHeader::KEY_ONE_BITS | VarHeader::SEQ_ONE_BITS,
                    0x00,
                    0x00,
                ],
            ),
            (
                VarHeader {
                    key: VarKey::Key1(Key1(1)),
                    seq_no: VarSeq::Seq1(0x02),
                },
                &[
                    VarHeader::KEY_ONE_BITS | VarHeader::SEQ_ONE_BITS,
                    0x01,
                    0x02,
                ],
            ),
            (
                VarHeader {
                    key: VarKey::Key2(Key2([0x42, 0xAF])),
                    seq_no: VarSeq::Seq1(0x02),
                },
                &[
                    VarHeader::KEY_TWO_BITS | VarHeader::SEQ_ONE_BITS,
                    0x42,
                    0xAF,
                    0x02,
                ],
            ),
            (
                VarHeader {
                    key: VarKey::Key1(Key1(1)),
                    seq_no: VarSeq::Seq2(0x42_AF),
                },
                &[
                    VarHeader::KEY_ONE_BITS | VarHeader::SEQ_TWO_BITS,
                    0x01,
                    0xAF,
                    0x42,
                ],
            ),
            (
                VarHeader {
                    key: VarKey::Key8(Key([0x12, 0x23, 0x34, 0x45, 0x56, 0x67, 0x78, 0x89])),
                    seq_no: VarSeq::Seq4(0x42_AF_AA_BB),
                },
                &[
                    VarHeader::KEY_EIGHT_BITS | VarHeader::SEQ_FOUR_BITS,
                    0x12,
                    0x23,
                    0x34,
                    0x45,
                    0x56,
                    0x67,
                    0x78,
                    0x89,
                    0xBB,
                    0xAA,
                    0xAF,
                    0x42,
                ],
            ),
        ];

        let mut buf = [0u8; 1 + 8 + 4];

        for (val, exp) in checks.iter() {
            let (used, _) = val.write_to_slice(&mut buf).unwrap();
            assert_eq!(used, *exp);
            let v = val.write_to_vec();
            assert_eq!(&v, *exp);
            let (deser, remain) = VarHeader::take_from_slice(used).unwrap();
            assert!(remain.is_empty());
            assert_eq!(val, &deser);
        }
    }
}
