use super::*;
#[test] fn markup_that_would_start_a_list_or_heading_is_escaped() {
    assert_eq!(escape_markup("- item", true), "\\- item");
    assert_eq!(escape_markup("1. first", true), "1\\. first");
    assert_eq!(escape_markup("a - b -- c", true), "a - b \\-- c");
    assert_eq!(escape_markup("say \"hi\"", false), "say \\\"hi\\\"");
}
#[test] fn string_escaping_cannot_terminate_a_typst_argument() {
    assert_eq!(typst_string("a\"\\\n\t"), "\"a\\\"\\\\\\n\\t\"");
}
