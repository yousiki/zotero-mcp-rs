use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::shared::types::{ZoteroCreator, ZoteroItemData};

pub const DEFAULT_RENAME_TEMPLATE: &str =
    "{{ firstCreator suffix=\" - \" }}{{ year suffix=\" - \" }}{{ title truncate=\"100\" }}";

#[derive(Debug, Clone)]
struct Chunk {
    value: String,
    raw_value: String,
    suffix: String,
    prefix: String,
}

#[derive(Debug, Default)]
struct CommonContext {
    chunks: Vec<Chunk>,
    protected_literals: HashSet<String>,
}

#[derive(Debug, Clone)]
enum Operand {
    Statement(String),
    Quoted(String),
    Bare(String),
}

fn html_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]+>").unwrap())
}

fn year_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(\d{4})\b").unwrap())
}

fn numeric_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[+-]?\d+(\.\d+)?$").unwrap())
}

pub fn hyphen_to_camel(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut uppercase_next = false;

    for ch in value.chars() {
        if ch == '-' {
            uppercase_next = true;
        } else if uppercase_next {
            result.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

pub fn strip_html_tags(html: &str) -> String {
    html_tag_regex().replace_all(html, "").into_owned()
}

pub fn get_valid_file_name(file_name: &str) -> String {
    let mut cleaned = String::with_capacity(file_name.len());

    for ch in file_name.chars() {
        match ch {
            '/' | '\\' | '?' | '*' | ':' | '|' | '"' | '<' | '>' => {}
            '\r' | '\n' | '\t' | '\u{2028}' | '\u{2029}' => cleaned.push(' '),
            '\u{2000}'..='\u{200A}' => cleaned.push(' '),
            '\u{200B}'..='\u{200E}' | '\u{2068}' | '\u{2069}' => {}
            '\u{0000}'..='\u{0008}' | '\u{000B}' | '\u{000C}' | '\u{000E}'..='\u{001F}' => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            _ => cleaned.push(ch),
        }
    }

    while cleaned.starts_with('.') {
        cleaned.remove(0);
    }

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "_".to_string()
    } else {
        cleaned
    }
}

pub fn split_by_outer_brackets(input: &str, left: &str, right: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut start_index = 0usize;
    let mut depth = 0usize;
    let mut i = 0usize;

    while i < input.len() {
        if input[i..].starts_with(left) {
            if depth == 0 {
                result.push(input[start_index..i].to_string());
                start_index = i;
            }
            depth += 1;
            i += left.len();
            continue;
        }

        if input[i..].starts_with(right) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                let end = i + right.len();
                result.push(input[start_index..end].to_string());
                start_index = end;
            }
            i += right.len();
            continue;
        }

        i += 1;
    }

    if start_index < input.len() {
        result.push(input[start_index..].to_string());
    }

    result
}

fn get_attributes(part: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let bytes = part.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        while i < bytes.len()
            && !(bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
        {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let key_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
        {
            i += 1;
        }
        let key = &part[key_start..i];

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
            continue;
        }

        let quote = bytes[i] as char;
        i += 1;
        let mut value = String::new();

        while i < bytes.len() {
            let ch = part[i..].chars().next().unwrap();
            let ch_len = ch.len_utf8();
            if ch == '\\' {
                let next_index = i + ch_len;
                if next_index < bytes.len() {
                    let next = part[next_index..].chars().next().unwrap();
                    if next == quote {
                        value.push(next);
                        i = next_index + next.len_utf8();
                        continue;
                    }
                }
            }
            if ch == quote {
                i += ch_len;
                break;
            }

            value.push(ch);
            i += ch_len;
        }

        attrs.insert(hyphen_to_camel(key), value);
    }

    attrs
}

fn split_statement(statement: &str) -> (String, String) {
    let inner = statement
        .trim()
        .trim_start_matches("{{")
        .trim_end_matches("}}")
        .trim();

    let mut parts = inner.splitn(2, char::is_whitespace);
    let operator = parts.next().unwrap_or("").trim().to_string();
    let args = parts.next().unwrap_or("").trim().to_string();
    (operator, args)
}

fn as_number(value: &str) -> Option<f64> {
    if numeric_regex().is_match(value.trim()) {
        value.trim().parse::<f64>().ok()
    } else {
        None
    }
}

fn capitalize_title(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut capitalize_next = true;

    for ch in value.chars() {
        if capitalize_next && ch.is_alphanumeric() {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
            capitalize_next = !ch.is_alphanumeric();
        }
    }

    result
}

fn normalize_joined_case(value: &str, separator: char) -> String {
    let mut result = String::new();
    let mut prev_separator = false;

    for ch in value.to_lowercase().chars() {
        if ch.is_whitespace() || ch == separator {
            if !result.is_empty() && !prev_separator {
                result.push(separator);
                prev_separator = true;
            }
        } else {
            result.push(ch);
            prev_separator = false;
        }
    }

    result.trim_matches(separator).to_string()
}

fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

pub fn apply_text_case(value: &str, text_case: &str) -> String {
    match text_case {
        "upper" => value.to_uppercase(),
        "lower" => value.to_lowercase(),
        "sentence" => {
            let mut chars = value.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        "title" => capitalize_title(value),
        "hyphen" => normalize_joined_case(value, '-'),
        "snake" => normalize_joined_case(value, '_'),
        "camel" => {
            let words = split_words(&value.to_lowercase());
            let mut result = String::new();
            for (index, word) in words.iter().enumerate() {
                if index == 0 {
                    result.push_str(word);
                } else {
                    let mut chars = word.chars();
                    if let Some(first) = chars.next() {
                        result.extend(first.to_uppercase());
                        result.push_str(chars.as_str());
                    }
                }
            }
            result
        }
        "pascal" => {
            let camel = apply_text_case(value, "camel");
            let mut chars = camel.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        _ => value.to_string(),
    }
}

fn compile_regex(pattern: &str, regex_opts: &str) -> Option<Regex> {
    let mut flags = String::new();
    if regex_opts.contains('i') {
        flags.push('i');
    }
    if regex_opts.contains('v') {
        flags.push('x');
    }

    let wrapped = if flags.is_empty() {
        pattern.to_string()
    } else {
        format!("(?{flags}){pattern}")
    };

    Regex::new(&wrapped).ok()
}

fn protect_literals(value: &str, protected_literals: &HashSet<String>) -> String {
    let mut protected = value.to_string();
    for literal in protected_literals {
        protected = protected.replace(literal, &format!(r"\{}//", literal));
    }
    protected
}

fn apply_common(
    value: Option<&str>,
    attrs: &HashMap<String, String>,
    context: Option<&mut CommonContext>,
) -> String {
    let Some(initial) = value else {
        return String::new();
    };
    if initial.is_empty() {
        return String::new();
    }

    let mut value = initial.to_string();
    let mut prefix = attrs.get("prefix").cloned().unwrap_or_default();
    let mut suffix = attrs.get("suffix").cloned().unwrap_or_default();

    let start_str = attrs.get("start").cloned().unwrap_or_default();
    let truncate_str = attrs.get("truncate").cloned().unwrap_or_default();
    let match_str = attrs.get("match").cloned().unwrap_or_default();
    let replace_from = attrs.get("replaceFrom").cloned().unwrap_or_default();
    let replace_to = attrs.get("replaceTo").cloned().unwrap_or_default();
    let regex_opts = attrs
        .get("regexOpts")
        .cloned()
        .unwrap_or_else(|| "vi".to_string());
    let text_case = attrs.get("case").cloned().unwrap_or_default();

    if prefix == r"\" || prefix == "/" {
        prefix.clear();
    }
    if suffix == r"\" || suffix == "/" {
        suffix.clear();
    }

    if !match_str.is_empty() {
        return compile_regex(&match_str, &regex_opts)
            .and_then(|re| re.find(&value).map(|m| m.as_str().to_string()))
            .unwrap_or_default();
    }

    if let Some(ctx) = context.as_ref()
        && !ctx.protected_literals.is_empty()
    {
        value = protect_literals(&value, &ctx.protected_literals);
    }

    let start = start_str.parse::<usize>().unwrap_or(0);
    let truncate = truncate_str.parse::<usize>().unwrap_or(0);

    if start > 0 {
        value = value.chars().skip(start).collect();
    }
    if truncate > 0 {
        value = value.chars().take(truncate).collect();
    }

    value = value.trim().to_string();
    let raw_value = value.clone();
    let mut affixed = false;

    if !replace_from.is_empty()
        && let Some(_re) = compile_regex(&replace_from, &regex_opts)
    {
        value = _re.replace_all(&value, replace_to.as_str()).into_owned();
    }

    if !prefix.is_empty() && !value.starts_with(&prefix) {
        value = format!("{prefix}{value}");
        affixed = true;
    }
    if !suffix.is_empty() && !value.ends_with(&suffix) {
        value.push_str(&suffix);
        affixed = true;
    }

    if affixed && let Some(ctx) = context {
        ctx.chunks.push(Chunk {
            value: value.clone(),
            raw_value,
            suffix: suffix.clone(),
            prefix: prefix.clone(),
        });
    }

    if !text_case.is_empty() {
        value = apply_text_case(&value, &text_case);
    }

    value
}

pub fn compute_first_creator(creators: &[ZoteroCreator]) -> String {
    if creators.is_empty() {
        return String::new();
    }

    let authors: Vec<&ZoteroCreator> = creators
        .iter()
        .filter(|creator| creator.creator_type == "author")
        .collect();
    let effective = if authors.is_empty() {
        creators.iter().collect::<Vec<_>>()
    } else {
        authors
    };

    let get_name = |creator: &ZoteroCreator| {
        creator
            .last_name
            .as_deref()
            .or(creator.name.as_deref())
            .unwrap_or_default()
            .to_string()
    };

    match effective.len() {
        0 => String::new(),
        1 => get_name(effective[0]),
        2 => format!("{} and {}", get_name(effective[0]), get_name(effective[1])),
        _ => format!("{} et al.", get_name(effective[0])),
    }
}

fn initialize_name(name: Option<&str>, should_initialize: bool, initialize_with: &str) -> String {
    let Some(name) = name else {
        return String::new();
    };

    if should_initialize {
        match name.chars().next() {
            Some(first) => format!("{}{}", first.to_uppercase(), initialize_with),
            None => String::new(),
        }
    } else {
        name.to_string()
    }
}

fn transform_name(creator: &ZoteroCreator, attrs: &HashMap<String, String>) -> String {
    let name_format = attrs.get("name").map(String::as_str).unwrap_or("family");
    let name_part_separator = attrs
        .get("namePartSeparator")
        .map(String::as_str)
        .unwrap_or(" ");
    let initialize = attrs.get("initialize").map(String::as_str).unwrap_or("");
    let initialize_with = attrs
        .get("initializeWith")
        .map(String::as_str)
        .unwrap_or(".");

    if let Some(name) = creator.name.as_deref() {
        let should_initialize = matches!(initialize, "full" | "name");
        return initialize_name(Some(name), should_initialize, initialize_with);
    }

    let first_last = ["full", "given-family", "first-last"];
    let last_first = ["full-reversed", "family-given", "last-first"];
    let first = ["given", "first"];
    let last = ["family", "last"];

    if first_last.contains(&name_format) {
        return format!(
            "{}{}{}",
            initialize_name(
                creator.first_name.as_deref(),
                ["full"]
                    .into_iter()
                    .chain(first)
                    .any(|value| value == initialize),
                initialize_with,
            ),
            name_part_separator,
            initialize_name(
                creator.last_name.as_deref(),
                ["full"]
                    .into_iter()
                    .chain(last)
                    .any(|value| value == initialize),
                initialize_with,
            )
        );
    }

    if last_first.contains(&name_format) {
        return format!(
            "{}{}{}",
            initialize_name(
                creator.last_name.as_deref(),
                ["full"]
                    .into_iter()
                    .chain(last)
                    .any(|value| value == initialize),
                initialize_with,
            ),
            name_part_separator,
            initialize_name(
                creator.first_name.as_deref(),
                ["full"]
                    .into_iter()
                    .chain(first)
                    .any(|value| value == initialize),
                initialize_with,
            )
        );
    }

    if first.contains(&name_format) {
        return initialize_name(
            creator.first_name.as_deref(),
            ["full"]
                .into_iter()
                .chain(first)
                .any(|value| value == initialize),
            initialize_with,
        );
    }

    initialize_name(
        creator.last_name.as_deref(),
        ["full"]
            .into_iter()
            .chain(last)
            .any(|value| value == initialize),
        initialize_with,
    )
}

fn get_creators_of_type(
    creators: &[ZoteroCreator],
    creator_type: &str,
    max: i32,
) -> Vec<ZoteroCreator> {
    let filtered: Vec<ZoteroCreator> = match creator_type {
        "authors" => creators
            .iter()
            .filter(|creator| creator.creator_type == "author")
            .cloned()
            .collect(),
        "editors" => creators
            .iter()
            .filter(|creator| {
                creator.creator_type == "editor" || creator.creator_type == "seriesEditor"
            })
            .cloned()
            .collect(),
        _ => creators.to_vec(),
    };

    if max == 0 {
        return Vec::new();
    }
    if max > 0 {
        let limit = max as usize;
        if limit >= filtered.len() {
            return filtered;
        }
        return filtered.into_iter().take(limit).collect();
    }

    let start = filtered.len().saturating_sub((-max) as usize);
    let mut sliced: Vec<ZoteroCreator> = filtered.into_iter().skip(start).collect();
    sliced.reverse();
    sliced
}

pub fn format_creator_list(
    creators: &[ZoteroCreator],
    creator_type: &str,
    attrs: &HashMap<String, String>,
) -> String {
    let max = attrs
        .get("max")
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(i32::MAX);
    let join = attrs.get("join").map(String::as_str).unwrap_or(", ");

    get_creators_of_type(creators, creator_type, max)
        .iter()
        .map(|creator| transform_name(creator, attrs))
        .collect::<Vec<_>>()
        .join(join)
}

pub fn extract_year(date: Option<&str>) -> String {
    let Some(date) = date else {
        return String::new();
    };

    year_regex()
        .captures(date)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .filter(|year| year != "0000")
        .unwrap_or_default()
}

struct TemplateEngine<'a> {
    item: &'a ZoteroItemData,
    attachment_title: &'a str,
}

impl<'a> TemplateEngine<'a> {
    fn new(item: &'a ZoteroItemData, attachment_title: &'a str) -> Self {
        Self {
            item,
            attachment_title,
        }
    }

    fn evaluate_identifier(
        &self,
        ident: &str,
        attrs: &HashMap<String, String>,
        context: &mut CommonContext,
    ) -> String {
        let ident = hyphen_to_camel(ident.trim());
        let creators = self.item.creators.as_deref().unwrap_or(&[]);

        match ident.as_str() {
            "authors" | "editors" | "creators" => {
                let value = format_creator_list(creators, ident.as_str(), attrs);
                apply_common(Some(&value), attrs, Some(context))
            }
            "authorsCount" | "editorsCount" | "creatorsCount" => {
                let kind = ident.trim_end_matches("Count");
                let count = get_creators_of_type(creators, kind, i32::MAX)
                    .len()
                    .to_string();
                apply_common(Some(&count), attrs, Some(context))
            }
            "firstCreator" => {
                apply_common(Some(&compute_first_creator(creators)), attrs, Some(context))
            }
            "year" => apply_common(
                Some(&extract_year(self.item.date.as_deref())),
                attrs,
                Some(context),
            ),
            "itemType" => apply_common(Some(&self.item.item_type), attrs, Some(context)),
            "attachmentTitle" => apply_common(Some(self.attachment_title), attrs, Some(context)),
            "accessDate" => apply_common(
                self.lookup_field("accessDate").as_deref(),
                attrs,
                Some(context),
            ),
            _ => {
                let value = self.lookup_field(&ident);
                apply_common(value.as_deref(), attrs, Some(context))
            }
        }
    }

    fn lookup_field(&self, ident: &str) -> Option<String> {
        match ident {
            "title" => self.item.title.clone(),
            "date" => self.item.date.clone(),
            "dateAdded" => self.item.date_added.clone(),
            "dateModified" => self.item.date_modified.clone(),
            "abstractNote" => self.item.abstract_note.clone(),
            "publicationTitle" => self.item.publication_title.clone(),
            "volume" => self.item.volume.clone(),
            "issue" => self.item.issue.clone(),
            "pages" => self.item.pages.clone(),
            "doi" => self.item.doi.clone(),
            "url" => self.item.url.clone(),
            "publisher" => self.item.publisher.clone(),
            "place" => self.item.place.clone(),
            "issn" => self.item.issn.clone(),
            "extra" => self.item.extra.clone(),
            "parentItem" => self.item.parent_item.clone(),
            "note" => self.item.note.clone(),
            "contentType" => self.item.content_type.clone(),
            "filename" => self.item.filename.clone(),
            "md5" => self.item.md5.clone(),
            "annotationType" => self.item.annotation_type.clone(),
            "annotationText" => self.item.annotation_text.clone(),
            "annotationComment" => self.item.annotation_comment.clone(),
            "annotationColor" => self.item.annotation_color.clone(),
            "annotationPageLabel" => self.item.annotation_page_label.clone(),
            "annotationPosition" => self.item.annotation_position.clone(),
            "linkMode" => self.item.link_mode.clone(),
            _ => self.lookup_extra_field(ident),
        }
    }

    fn lookup_extra_field(&self, ident: &str) -> Option<String> {
        self.item
            .extra_fields
            .get(ident)
            .and_then(|value| match value {
                Value::String(text) => Some(text.clone()),
                _ => None,
            })
    }

    fn evaluate_statement(&self, statement: &str, context: &mut CommonContext) -> String {
        let statement = statement.trim();
        let operator = statement.split_whitespace().next().unwrap_or("").trim();
        let args = statement.get(operator.len()..).unwrap_or("").trim();
        self.evaluate_identifier(operator, &get_attributes(args), context)
    }

    fn evaluate_operand(&self, operand: Operand, context: &mut CommonContext) -> String {
        match operand {
            Operand::Statement(statement) => self.evaluate_statement(statement.trim(), context),
            Operand::Quoted(value) => value,
            Operand::Bare(value) => {
                if as_number(&value).is_some() {
                    value
                } else {
                    self.evaluate_identifier(&value, &HashMap::new(), context)
                }
            }
        }
    }

    fn evaluate_condition(&self, condition: &str, context: &mut CommonContext) -> bool {
        let trimmed = condition.trim();
        let comparators = ["==", "!=", "<=", ">=", "<", ">"];

        if let Some((left, comparator, right)) = split_comparison(trimmed, &comparators) {
            let left_value = self.evaluate_operand(left, context);
            let right_value = self.evaluate_operand(right, context);

            let left_num = as_number(&left_value);
            let right_num = as_number(&right_value);
            let left_lower = left_value.to_lowercase();
            let right_lower = right_value.to_lowercase();

            return match comparator.as_str() {
                "==" => {
                    if let (Some(left_num), Some(right_num)) = (left_num, right_num) {
                        left_num == right_num
                    } else {
                        left_lower == right_lower
                    }
                }
                "!=" => {
                    if let (Some(left_num), Some(right_num)) = (left_num, right_num) {
                        left_num != right_num
                    } else {
                        left_lower != right_lower
                    }
                }
                ">=" => {
                    if let (Some(left_num), Some(right_num)) = (left_num, right_num) {
                        left_num >= right_num
                    } else {
                        left_lower >= right_lower
                    }
                }
                "<=" => {
                    if let (Some(left_num), Some(right_num)) = (left_num, right_num) {
                        left_num <= right_num
                    } else {
                        left_lower <= right_lower
                    }
                }
                ">" => {
                    if let (Some(left_num), Some(right_num)) = (left_num, right_num) {
                        left_num > right_num
                    } else {
                        left_lower > right_lower
                    }
                }
                "<" => {
                    if let (Some(left_num), Some(right_num)) = (left_num, right_num) {
                        left_num < right_num
                    } else {
                        left_lower < right_lower
                    }
                }
                _ => false,
            };
        }

        if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
            let (operator, args) = split_statement(trimmed);
            return !self
                .evaluate_identifier(&operator, &get_attributes(&args), context)
                .is_empty();
        }

        !self
            .evaluate_identifier(trimmed, &HashMap::new(), context)
            .is_empty()
    }

    fn generate_from_template(&self, template: &str, context: &mut CommonContext) -> String {
        let mut html = String::new();
        let mut levels = vec![ConditionalLevel {
            condition: true,
            executed: true,
            parent_condition: true,
        }];

        for part in split_by_outer_brackets(template, "{{", "}}") {
            let current = levels.last_mut().unwrap();
            if part.starts_with("{{") {
                let (operator, args) = split_statement(&part);
                match operator.as_str() {
                    "if" => {
                        let parent_condition = current.condition;
                        let new_condition =
                            parent_condition && self.evaluate_condition(&args, context);
                        levels.push(ConditionalLevel {
                            condition: new_condition,
                            executed: new_condition,
                            parent_condition,
                        });
                    }
                    "elseif" => {
                        if !current.executed {
                            current.condition =
                                current.parent_condition && self.evaluate_condition(&args, context);
                            current.executed = current.condition;
                        } else {
                            current.condition = false;
                        }
                    }
                    "else" => {
                        current.condition = current.parent_condition && !current.executed;
                        current.executed = current.condition;
                    }
                    "endif" => {
                        if levels.len() > 1 {
                            levels.pop();
                        }
                    }
                    _ => {
                        if current.condition {
                            html.push_str(&self.evaluate_identifier(
                                &operator,
                                &get_attributes(&part),
                                context,
                            ));
                        }
                    }
                }
            } else if current.condition {
                html.push_str(&part);
            }
        }

        html
    }
}

#[derive(Debug, Clone, Copy)]
struct ConditionalLevel {
    condition: bool,
    executed: bool,
    parent_condition: bool,
}

fn parse_quoted(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let first = trimmed.chars().next()?;
    if first != '"' && first != '\'' {
        return None;
    }
    if !trimmed.ends_with(first) || trimmed.len() < 2 {
        return None;
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    let escaped = format!(r"\{first}");
    Some(inner.replace(&escaped, &first.to_string()))
}

fn parse_operand(input: &str) -> Operand {
    let trimmed = input.trim();
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        let (operator, args) = split_statement(trimmed);
        if args.is_empty() {
            Operand::Statement(operator)
        } else {
            Operand::Statement(format!("{operator} {args}"))
        }
    } else if let Some(quoted) = parse_quoted(trimmed) {
        Operand::Quoted(quoted)
    } else {
        Operand::Bare(trimmed.to_string())
    }
}

fn split_comparison(condition: &str, comparators: &[&str]) -> Option<(Operand, String, Operand)> {
    let bytes = condition.as_bytes();
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    let mut brace_depth = 0usize;

    while i < bytes.len() {
        let byte = bytes[i];

        if let Some(active_quote) = quote {
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == active_quote {
                quote = None;
            }
            i += 1;
            continue;
        }

        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            i += 1;
            continue;
        }
        if condition[i..].starts_with("{{") {
            brace_depth += 1;
            i += 2;
            continue;
        }
        if condition[i..].starts_with("}}") {
            brace_depth = brace_depth.saturating_sub(1);
            i += 2;
            continue;
        }

        if brace_depth == 0 {
            for comparator in comparators {
                if condition[i..].starts_with(comparator) {
                    let left = condition[..i].trim();
                    let right = condition[i + comparator.len()..].trim();
                    return Some((
                        parse_operand(left),
                        (*comparator).to_string(),
                        parse_operand(right),
                    ));
                }
            }
        }

        i += 1;
    }

    None
}

pub fn get_file_base_name_from_item(item: &ZoteroItemData, template: Option<&str>) -> String {
    let mut format_string = template
        .unwrap_or(DEFAULT_RENAME_TEMPLATE)
        .replace(['\r', '\n'], "");
    format_string = format_string.trim().to_string();

    let engine = TemplateEngine::new(item, "");
    let mut context = CommonContext::default();
    let mut formatted = engine.generate_from_template(&format_string, &mut context);

    let mut replace_pairs: HashMap<String, String> = HashMap::new();
    for chunk in &context.chunks {
        if !chunk.suffix.is_empty() {
            let duplicate = format!("{}{}{}", chunk.raw_value, chunk.suffix, chunk.suffix);
            if formatted.contains(&duplicate) {
                context.protected_literals.insert(duplicate.clone());
                replace_pairs.insert(duplicate, format!("{}{}", chunk.raw_value, chunk.suffix));
            }
        }
        if !chunk.prefix.is_empty() {
            let duplicate = format!("{}{}{}", chunk.prefix, chunk.prefix, chunk.raw_value);
            if formatted.contains(&duplicate) {
                context.protected_literals.insert(duplicate.clone());
                replace_pairs.insert(duplicate, format!("{}{}", chunk.prefix, chunk.raw_value));
            }
        }
        let _ = &chunk.value;
    }

    if !context.protected_literals.is_empty() {
        context.chunks.clear();
        for literal in context.protected_literals.clone() {
            format_string = format_string.replace(&literal, &format!(r"\{}//", literal));
        }
        formatted = engine.generate_from_template(&format_string, &mut context);

        let mut protected_markers: Vec<(String, String)> = Vec::new();
        for (idx, key) in replace_pairs.keys().enumerate() {
            let marker = format!("__PROTECTED_LITERAL_{idx}__");
            formatted = formatted.replace(&format!(r"\{}//", key), &marker);
            protected_markers.push((marker, key.clone()));
        }

        for (key, value) in &replace_pairs {
            formatted = formatted.replace(key, value);
        }

        for (marker, original) in protected_markers {
            formatted = formatted.replace(&marker, &original);
        }
    }

    get_valid_file_name(&strip_html_tags(&formatted))
}

pub fn build_renamed_filename(item: &ZoteroItemData, ext: &str) -> String {
    let base_name = get_file_base_name_from_item(item, None);
    if ext.is_empty() {
        base_name
    } else {
        format!("{base_name}.{ext}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::types::{ZoteroCreator, ZoteroItemData};

    fn creator(creator_type: &str, first_name: &str, last_name: &str) -> ZoteroCreator {
        ZoteroCreator {
            creator_type: creator_type.to_string(),
            first_name: Some(first_name.to_string()),
            last_name: Some(last_name.to_string()),
            name: None,
        }
    }

    fn single_name_creator(creator_type: &str, name: &str) -> ZoteroCreator {
        ZoteroCreator {
            creator_type: creator_type.to_string(),
            first_name: None,
            last_name: None,
            name: Some(name.to_string()),
        }
    }

    fn item_with(
        title: Option<&str>,
        creators: Vec<ZoteroCreator>,
        date: Option<&str>,
    ) -> ZoteroItemData {
        ZoteroItemData {
            item_type: "journalArticle".to_string(),
            title: title.map(str::to_string),
            creators: Some(creators),
            date: date.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn default_template_with_full_item() {
        let item = item_with(
            Some("Some Paper Title"),
            vec![creator("author", "Jane", "Smith")],
            Some("2024-01-15"),
        );

        assert_eq!(
            build_renamed_filename(&item, "pdf"),
            "Smith - 2024 - Some Paper Title.pdf"
        );
    }

    #[test]
    fn default_template_with_missing_creator() {
        let item = item_with(Some("Some Paper Title"), vec![], Some("2024-01-15"));
        assert_eq!(
            get_file_base_name_from_item(&item, None),
            "2024 - Some Paper Title"
        );
    }

    #[test]
    fn default_template_with_missing_year() {
        let item = item_with(
            Some("Some Paper Title"),
            vec![creator("author", "Jane", "Smith")],
            None,
        );

        assert_eq!(
            get_file_base_name_from_item(&item, None),
            "Smith - Some Paper Title"
        );
    }

    #[test]
    fn default_template_with_missing_creator_and_year() {
        let item = item_with(Some("Some Paper Title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, None),
            "Some Paper Title"
        );
    }

    #[test]
    fn truncates_default_title_to_100_chars() {
        let long_title = "a".repeat(120);
        let item = item_with(Some(&long_title), vec![], None);

        assert_eq!(get_file_base_name_from_item(&item, None), "a".repeat(100));
    }

    #[test]
    fn applies_upper_case() {
        let item = item_with(Some("Some Paper Title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"upper\" }}")),
            "SOME PAPER TITLE"
        );
    }

    #[test]
    fn applies_lower_case() {
        let item = item_with(Some("Some Paper Title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"lower\" }}")),
            "some paper title"
        );
    }

    #[test]
    fn applies_title_case() {
        let item = item_with(Some("some paper title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"title\" }}")),
            "Some Paper Title"
        );
    }

    #[test]
    fn applies_sentence_case() {
        let item = item_with(Some("some paper title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"sentence\" }}")),
            "Some paper title"
        );
    }

    #[test]
    fn applies_hyphen_case() {
        let item = item_with(Some("Some   Paper Title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"hyphen\" }}")),
            "some-paper-title"
        );
    }

    #[test]
    fn applies_snake_case() {
        let item = item_with(Some("Some   Paper Title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"snake\" }}")),
            "some_paper_title"
        );
    }

    #[test]
    fn applies_camel_case() {
        let item = item_with(Some("Some paper title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"camel\" }}")),
            "somePaperTitle"
        );
    }

    #[test]
    fn applies_pascal_case() {
        let item = item_with(Some("Some paper title"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title case=\"pascal\" }}")),
            "SomePaperTitle"
        );
    }

    #[test]
    fn evaluates_if_elseif_else_blocks() {
        let item = item_with(Some("Paper"), vec![], Some("2023-01-01"));
        let template =
            "{{ if year >= 2024 }}new{{ elseif year == 2023 }}old{{ else }}none{{ endif }}";
        assert_eq!(get_file_base_name_from_item(&item, Some(template)), "old");
    }

    #[test]
    fn computes_first_creator_for_one_author() {
        assert_eq!(
            compute_first_creator(&[creator("author", "Jane", "Smith")]),
            "Smith"
        );
    }

    #[test]
    fn computes_first_creator_for_two_authors() {
        assert_eq!(
            compute_first_creator(&[
                creator("author", "Jane", "Smith"),
                creator("author", "John", "Jones"),
            ]),
            "Smith and Jones"
        );
    }

    #[test]
    fn computes_first_creator_for_three_authors() {
        assert_eq!(
            compute_first_creator(&[
                creator("author", "Jane", "Smith"),
                creator("author", "John", "Jones"),
                creator("author", "Ada", "Brown"),
            ]),
            "Smith et al."
        );
    }

    #[test]
    fn strips_invalid_file_name_characters() {
        assert_eq!(get_valid_file_name("./bad:/\\name?*|<>\n"), "badname ");
    }

    #[test]
    fn extracts_year_from_date() {
        assert_eq!(extract_year(Some("Spring 2024")), "2024");
        assert_eq!(extract_year(Some("0000-02-02")), "");
    }

    #[test]
    fn formats_creator_lists_with_options() {
        let item = item_with(
            Some("Paper"),
            vec![
                creator("author", "Jane", "Smith"),
                creator("author", "John", "Jones"),
                creator("editor", "Ed", "Miles"),
            ],
            None,
        );

        let template =
            "{{ authors max=\"2\" name=\"given-family\" initialize=\"given\" join=\"; \" }}";
        assert_eq!(
            get_file_base_name_from_item(&item, Some(template)),
            "J. Smith; J. Jones"
        );
    }

    #[test]
    fn supports_truthy_conditionals() {
        let item = item_with(Some("Paper"), vec![], None);
        let template = "{{ if title }}yes{{ else }}no{{ endif }}";
        assert_eq!(get_file_base_name_from_item(&item, Some(template)), "yes");
    }

    #[test]
    fn supports_regex_match_and_replace() {
        let item = item_with(Some("A Study (Preprint)"), vec![], None);
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ title match=\"\\([^)]+\\)\" }}")),
            "(Preprint)"
        );
        assert_eq!(
            get_file_base_name_from_item(
                &item,
                Some("{{ title replaceFrom=\"\\s*\\([^)]+\\)\" replaceTo=\"\" }}")
            ),
            "A Study"
        );
    }

    #[test]
    fn deduplicates_duplicate_suffixes() {
        let item = item_with(Some("Paper"), vec![], Some("2024"));
        let template = "{{ year suffix=\" - \" }}{{ title prefix=\" - \" }}";
        assert_eq!(
            get_file_base_name_from_item(&item, Some(template)),
            "2024 - Paper"
        );
    }

    #[test]
    fn supports_named_creator_entries() {
        let item = item_with(
            Some("Paper"),
            vec![single_name_creator("author", "WHO")],
            None,
        );
        assert_eq!(
            get_file_base_name_from_item(&item, Some("{{ firstCreator }}")),
            "WHO"
        );
    }
}
