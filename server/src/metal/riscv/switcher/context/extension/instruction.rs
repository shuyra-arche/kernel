//! Recognize only the F/D instructions whose disabled-state trap we own.
//! Reserved encodings and other illegal instructions keep the ordinary fault path.
pub(crate) fn float(word: u32) -> bool {
    let f3 = (word >> 12) & 7;
    if word & 3 != 3 {
        return match (word & 3, (word >> 13) & 7) {
            (0, 1 | 5) | (2, 5) => true,     // C.FLD / C.FSD / C.FSDSP
            (2, 1) => (word >> 7) & 31 != 0, // C.FLDSP, rd != 0
            _ => false,
        };
    }
    let rs2 = (word >> 20) & 31;
    let rounding = f3 <= 4 || f3 == 7;
    match word & 0x7f {
        0x07 | 0x27 => matches!(f3, 2 | 3), // FLW/FLD/FSW/FSD
        0x43 | 0x47 | 0x4b | 0x4f => (word >> 25) & 3 <= 1 && rounding,
        0x53 => match word >> 25 {
            0 | 1 | 4 | 5 | 8 | 9 | 12 | 13 => rounding,
            44 | 45 => rs2 == 0 && rounding,
            16 | 17 => f3 <= 2,
            20 | 21 => f3 <= 1,
            32 => rs2 == 1 && rounding,
            33 => rs2 == 0 && rounding,
            80 | 81 => f3 <= 2,
            96 | 97 | 104 | 105 => rs2 <= 3 && rounding,
            112 | 113 => rs2 == 0 && f3 <= 1,
            120 | 121 => rs2 == 0 && f3 == 0,
            _ => false,
        },
        0x73 => matches!(f3, 1 | 2 | 3 | 5 | 6 | 7) && matches!(word >> 20, 1..=3),
        _ => false,
    }
}

/// Vector major opcodes are distinct from scalar F/D memory widths. Reserved
/// arithmetic encodings may retry once, then fault with VS enabled; the PC is
/// never advanced and they cannot be mistaken for repeated first use.
pub(crate) fn vector(word: u32) -> bool {
    let f3 = (word >> 12) & 7;
    match word & 0x7f {
        0x57 if f3 == 7 => word >> 31 == 0 || word >> 30 == 3 || word >> 25 == 0x40,
        0x57 => true,
        0x07 | 0x27 => {
            if !matches!(f3, 0 | 5 | 6 | 7) || word & (1 << 28) != 0 {
                return false;
            }
            if (word >> 26) & 3 != 0 {
                return true;
            }
            match (word >> 20) & 31 {
                0 => true,
                8 => {
                    // Whole-register groups: unmasked, aligned power-of-two count.
                    let count = (word >> 29) + 1;
                    matches!(count, 1 | 2 | 4 | 8)
                        && word & (1 << 25) != 0
                        && ((word >> 7) & 31) % count == 0
                        && (word & 0x7f == 0x07 || f3 == 0)
                }
                11 => f3 == 0 && word >> 29 == 0 && word & (1 << 25) != 0,
                16 => word & 0x7f == 0x07,
                _ => false,
            }
        }
        0x73 if matches!(f3, 1 | 2 | 3 | 5 | 6 | 7) => match word >> 20 {
            8..=10 | 15 => true,
            0xc20..=0xc22 => matches!(f3, 2 | 3 | 6 | 7) && (word >> 15) & 31 == 0,
            _ => false,
        },
        _ => false,
    }
}

pub(crate) fn vector_float(word: u32) -> bool {
    word & 0x7f == 0x57 && matches!((word >> 12) & 7, 1 | 5)
}
