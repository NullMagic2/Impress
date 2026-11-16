use super::*;

fn words(text: &str) -> Vec<String> { text.split_whitespace().map(str::to_string).collect() }

#[test]
fn english_is_told_from_unaccented_portuguese() {
    assert!(reads_as_english(&words("The history of the town is told in the chapters that follow, with maps and notes for each period.")));
    let portuguese = words("A gramatica do portugues contemporaneo trata das formas que a lingua tem no Brasil e em Portugal, com exemplos de autores.");
    assert!(!reads_as_english(&portuguese));
    assert_eq!(guess_language(&portuguese), Some(("Portuguese", "por")));
}

