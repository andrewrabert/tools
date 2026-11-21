//! 3DS AES key material and the hardware key scrambler.
use std::fmt;

const SCRAMBLER: u128 = 0x1FF9E9AAC5FE0408024591DC5D52768A;

#[derive(Clone, Copy)]
pub enum KeySet {
    Retail,
    Dev,
}

impl KeySet {
    pub fn from_dev(dev: bool) -> Self {
        if dev { KeySet::Dev } else { KeySet::Retail }
    }

    fn keyx(self, slot: Keyslot) -> u128 {
        match (self, slot) {
            (KeySet::Retail, Keyslot::Slot18) => 0x82E9C9BEBFB8BDB875ECC0A07D474374,
            (KeySet::Retail, Keyslot::Slot1B) => 0x45AD04953992C7C893724A9A7BCE6182,
            (KeySet::Retail, Keyslot::Slot25) => 0xCEE7D8AB30C00DAE850EF5E382AC5AF3,
            (KeySet::Retail, Keyslot::Slot2C) => 0xB98E95CECA3E4D171F76A94DE934C053,
            (KeySet::Dev, Keyslot::Slot18) => 0x304BF1468372EE64115EBD4093D84276,
            (KeySet::Dev, Keyslot::Slot1B) => 0x6C8B2944A0726035F941DFC018524FB6,
            (KeySet::Dev, Keyslot::Slot25) => 0x81907A4B6F1B47323A677974CE4AD71B,
            (KeySet::Dev, Keyslot::Slot2C) => 0x510207515507CBB18E243DCB85E23A1D,
        }
    }

    pub fn normal_key(self, slot: Keyslot, keyy: KeyY) -> NormalKey {
        let keyx = self.keyx(slot);
        NormalKey(
            (keyx.rotate_left(2) ^ keyy.0)
                .wrapping_add(SCRAMBLER)
                .rotate_left(87),
        )
    }
}

#[derive(Clone, Copy)]
pub enum Keyslot {
    Slot18,
    Slot1B,
    Slot25,
    Slot2C,
}

impl fmt::Display for Keyslot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let id: u8 = match self {
            Keyslot::Slot18 => 0x18,
            Keyslot::Slot1B => 0x1B,
            Keyslot::Slot25 => 0x25,
            Keyslot::Slot2C => 0x2C,
        };
        write!(f, "0x{id:02X}")
    }
}

#[derive(Clone, Copy)]
pub struct KeyY(u128);

impl From<[u8; 16]> for KeyY {
    fn from(bytes: [u8; 16]) -> Self {
        KeyY(u128::from_be_bytes(bytes))
    }
}

#[derive(Clone, Copy)]
pub struct NormalKey(u128);

impl NormalKey {
    pub const ZERO: NormalKey = NormalKey(0);

    pub fn to_bytes(self) -> [u8; 16] {
        self.0.to_be_bytes()
    }
}
