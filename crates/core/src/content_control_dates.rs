//! Gregorian date values shared by content-control bindings and native pickers.
const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
pub fn control_date_parts(value: &str) -> Option<(i32, u32, u32)> {
    let date = value.split('T').next()?;
    let mut parts = date.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || year < 1 || year > 9999 || !(1..=12).contains(&month) {
        return None;
    }
    let limit = MONTH_DAYS[month as usize - 1]
        + u32::from(month == 2 && year % 4 == 0 && (year % 100 != 0 || year % 400 == 0));
    (day >= 1 && day <= limit).then_some((year, month, day))
}
pub fn control_date_display(value: &str, pattern: &str) -> Option<String> {
    control_date_display_language(value, pattern, None)
}
pub fn control_date_display_language(
    value: &str,
    pattern: &str,
    language: Option<&str>,
) -> Option<String> {
    let (year, month, day) = control_date_parts(value)?;
    let (months, weekdays) = date_names(language);
    let mut weekday_year = year;
    if month < 3 {
        weekday_year -= 1;
    }
    let weekday = (weekday_year + weekday_year / 4 - weekday_year / 100
        + weekday_year / 400
        + WEEKDAY_MONTH_OFFSETS[month as usize - 1]
        + day as i32)
        % 7;
    let mut result = String::new();
    let mut chars = pattern.chars().peekable();
    let mut quoted = None;
    while let Some(character) = chars.next() {
        if let Some(delimiter) = quoted {
            if character == delimiter {
                quoted = None;
            } else {
                result.push(character);
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quoted = Some(character);
            continue;
        }
        let mut count = 1;
        while chars.peek() == Some(&character) {
            chars.next();
            count += 1;
        }
        match (character, count) {
            ('y', 2) => result.push_str(&format!("{:02}", year % 100)),
            ('y', _) => result.push_str(&format!("{year:04}")),
            ('M', 1) => result.push_str(&month.to_string()),
            ('M', 2) => result.push_str(&format!("{month:02}")),
            ('d', 1) => result.push_str(&day.to_string()),
            ('d', 2) => result.push_str(&format!("{day:02}")),
            ('M', 3) => result.push_str(
                &months[month as usize - 1]
                    .chars()
                    .take(3)
                    .collect::<String>(),
            ),
            ('M', _) => result.push_str(months[month as usize - 1]),
            ('d', 3) => result.push_str(
                &weekdays[weekday as usize]
                    .chars()
                    .take(3)
                    .collect::<String>(),
            ),
            ('d', _) => result.push_str(weekdays[weekday as usize]),
            _ => {
                for _ in 0..count {
                    result.push(character);
                }
            }
        }
    }
    Some(result)
}

const WEEKDAY_MONTH_OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
const MONTH_NAMES_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_NAMES_EL: [&str; 12] = [
    "Ιανουαρίου",
    "Φεβρουαρίου",
    "Μαρτίου",
    "Απριλίου",
    "Μαΐου",
    "Ιουνίου",
    "Ιουλίου",
    "Αυγούστου",
    "Σεπτεμβρίου",
    "Οκτωβρίου",
    "Νοεμβρίου",
    "Δεκεμβρίου",
];
const MONTH_NAMES_ES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];
const MONTH_NAMES_PT: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];
const WEEKDAY_NAMES_EN: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const WEEKDAY_NAMES_EL: [&str; 7] = [
    "Κυριακή",
    "Δευτέρα",
    "Τρίτη",
    "Τετάρτη",
    "Πέμπτη",
    "Παρασκευή",
    "Σάββατο",
];
const WEEKDAY_NAMES_ES: [&str; 7] = [
    "domingo",
    "lunes",
    "martes",
    "miércoles",
    "jueves",
    "viernes",
    "sábado",
];
const WEEKDAY_NAMES_PT: [&str; 7] = [
    "domingo",
    "segunda-feira",
    "terça-feira",
    "quarta-feira",
    "quinta-feira",
    "sexta-feira",
    "sábado",
];
fn date_names(language: Option<&str>) -> (&'static [&'static str; 12], &'static [&'static str; 7]) {
    match language
        .unwrap_or("en")
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
    {
        "el" => (&MONTH_NAMES_EL, &WEEKDAY_NAMES_EL),
        "es" => (&MONTH_NAMES_ES, &WEEKDAY_NAMES_ES),
        "pt" => (&MONTH_NAMES_PT, &WEEKDAY_NAMES_PT),
        _ => (&MONTH_NAMES_EN, &WEEKDAY_NAMES_EN),
    }
}
