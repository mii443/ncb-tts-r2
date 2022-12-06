use regex::Regex;

pub fn remove_url(text: String) -> String {
    let url_regex = Regex::new(r"(http://|https://){1}[\w\.\-/:\#\?=\&;%\~\+]+").unwrap();
    let code_regex = Regex::new(r"```(.*)\n(.*)\n```").unwrap();
    let text = url_regex.replace_all(&text, " URL ").to_string();
    code_regex.replace_all(&text, "code").to_string()
}
