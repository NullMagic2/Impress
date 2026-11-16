use super::*;

#[test]
fn every_replacement_keeps_the_utf8_length() {
    for &(private, shown) in SYMBOL_FONT_CHARACTERS {
        assert_eq!(char::from_u32(private).unwrap().len_utf8(), shown.len_utf8(), "U+{private:04X}");
    }
    assert_eq!('\u{F020}'.len_utf8(), UNKNOWN_SYMBOL.len_utf8());
}
