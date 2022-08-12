use regex::Regex;

pub fn remove_url(text: String) -> String {
    let url_regex = Regex::new(r"(http://|https://){1}[\w\.\-/:\#\?=\&;%\~\+]+").unwrap();
    url_regex.replace_all(&text, " URL ").to_string()
}
