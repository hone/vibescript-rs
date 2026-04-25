use logos::Logos;
use std::fmt;

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Def,
    Do,
    If,
    Elsif,
    Else,
    End,
    While,
    Until,
    For,
    In,
    Break,
    Next,
    Case,
    When,
    Begin,
    Rescue,
    Ensure,
    Enum,
    Class,
    SelfToken,
    Property,
    Getter,
    Setter,
    Private,
    Return,
    Assert,
    True,
    False,
    Nil,
    Then,

    DotDot,
    Symbol(String),
    Ident(String),
    Ivar(String),
    Cvar(String),
    Int(i64),
    Float(f64),
    String(String), // Keep for legacy/simple strings if needed, but we'll prefer the sequence

    // New variants for full interpolation
    StringStart,
    StringEnd,
    StringText(String),
    InterpolationStart,
    InterpolationEnd,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Not,

    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    ColonColon,
    Dot,
    Pipe,
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\n\f]+")] // Skip whitespace
#[logos(skip(r"#[^\n]*", allow_greedy = true))] // Skip comments
enum NormalToken {
    #[token("def")]
    Def,
    #[token("do")]
    Do,
    #[token("if")]
    If,
    #[token("elsif")]
    Elsif,
    #[token("else")]
    Else,
    #[token("end")]
    End,
    #[token("while")]
    While,
    #[token("until")]
    Until,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("break")]
    Break,
    #[token("next")]
    Next,
    #[token("case")]
    Case,
    #[token("when")]
    When,
    #[token("begin")]
    Begin,
    #[token("rescue")]
    Rescue,
    #[token("ensure")]
    Ensure,
    #[token("enum")]
    Enum,
    #[token("class")]
    Class,
    #[token("self")]
    SelfToken,
    #[token("property")]
    Property,
    #[token("getter")]
    Getter,
    #[token("setter")]
    Setter,
    #[token("private")]
    Private,
    #[token("return")]
    Return,
    #[token("assert")]
    Assert,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("nil")]
    Nil,
    #[token("then")]
    Then,

    #[token("..")]
    DotDot,

    #[regex(":[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Symbol(String),

    #[regex("[a-zA-Z_][a-zA-Z0-9_]*[\\?!]?", |lex| lex.slice().to_string())]
    Ident(String),

    #[regex("@[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Ivar(String),

    #[regex("@@[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[2..].to_string())]
    Cvar(String),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Int(i64),

    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[token("\"")]
    Quote,

    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("=")]
    Assign,
    #[token("==")]
    Eq,
    #[token("!=")]
    NotEq,
    #[token("<")]
    Lt,
    #[token("<=")]
    LtEq,
    #[token(">")]
    Gt,
    #[token(">=")]
    GtEq,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("!")]
    Not,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token("::")]
    ColonColon,
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
}

#[derive(Logos, Debug, PartialEq, Clone)]
enum StringToken {
    #[token("#{")]
    InterpolationStart,

    #[token("\"")]
    Quote,

    #[regex(r#"\\[\\nrt"]"#, |lex| {
        let s = lex.slice();
        match s {
            "\\n" => "\n".to_string(),
            "\\r" => "\r".to_string(),
            "\\t" => "\t".to_string(),
            "\\\"" => "\"".to_string(),
            "\\\\" => "\\".to_string(),
            _ => s.to_string(),
        }
    })]
    Escape(String),

    #[regex(r#"[^"\\#]+"#, |lex| lex.slice().to_string())]
    #[token("#", |lex| lex.slice().to_string())]
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LexMode {
    Normal,
    String,
}

pub fn lex_with_spans(source: &str) -> (Vec<Token>, Vec<std::ops::Range<usize>>) {
    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut mode = LexMode::Normal;
    let mut brace_depth = 0;
    let mut offset = 0;

    while offset < source.len() {
        match mode {
            LexMode::Normal => {
                let mut lexer = NormalToken::lexer(&source[offset..]);
                if let Some(res) = lexer.next() {
                    let span = lexer.span();
                    let actual_span = (offset + span.start)..(offset + span.end);
                    match res {
                        Ok(NormalToken::Quote) => {
                            tokens.push(Token::StringStart);
                            spans.push(actual_span.clone());
                            mode = LexMode::String;
                        }
                        Ok(NormalToken::LBrace) => {
                            tokens.push(Token::LBrace);
                            spans.push(actual_span.clone());
                            brace_depth += 1;
                        }
                        Ok(NormalToken::RBrace) => {
                            if brace_depth > 0 {
                                tokens.push(Token::RBrace);
                                spans.push(actual_span.clone());
                                brace_depth -= 1;
                            } else {
                                // This might be the end of an interpolation
                                tokens.push(Token::InterpolationEnd);
                                spans.push(actual_span.clone());
                                mode = LexMode::String;
                            }
                        }
                        Ok(tok) => {
                            tokens.push(map_normal_token(tok));
                            spans.push(actual_span.clone());
                        }
                        Err(_) => {
                            // On error, we just push Nil or skip for now
                            // In a real implementation we'd handle errors
                            offset += 1;
                            continue;
                        }
                    }
                    offset += span.end;
                } else {
                    break;
                }
            }
            LexMode::String => {
                let mut lexer = StringToken::lexer(&source[offset..]);
                if let Some(res) = lexer.next() {
                    let span = lexer.span();
                    let actual_span = (offset + span.start)..(offset + span.end);
                    match res {
                        Ok(StringToken::Quote) => {
                            tokens.push(Token::StringEnd);
                            spans.push(actual_span.clone());
                            mode = LexMode::Normal;
                        }
                        Ok(StringToken::InterpolationStart) => {
                            tokens.push(Token::InterpolationStart);
                            spans.push(actual_span.clone());
                            mode = LexMode::Normal;
                        }
                        Ok(StringToken::Escape(s)) | Ok(StringToken::Text(s)) => {
                            // Consolidate consecutive text if possible
                            if let Some(Token::StringText(last)) = tokens.last_mut() {
                                last.push_str(&s);
                                let last_span = spans.last_mut().unwrap();
                                last_span.end = actual_span.end;
                            } else {
                                tokens.push(Token::StringText(s));
                                spans.push(actual_span.clone());
                            }
                        }
                        Err(_) => {
                            offset += 1;
                            continue;
                        }
                    }
                    offset += span.end;
                } else {
                    break;
                }
            }
        }
    }

    (tokens, spans)
}

fn map_normal_token(tok: NormalToken) -> Token {
    match tok {
        NormalToken::Def => Token::Def,
        NormalToken::Do => Token::Do,
        NormalToken::If => Token::If,
        NormalToken::Elsif => Token::Elsif,
        NormalToken::Else => Token::Else,
        NormalToken::End => Token::End,
        NormalToken::While => Token::While,
        NormalToken::Until => Token::Until,
        NormalToken::For => Token::For,
        NormalToken::In => Token::In,
        NormalToken::Break => Token::Break,
        NormalToken::Next => Token::Next,
        NormalToken::Case => Token::Case,
        NormalToken::When => Token::When,
        NormalToken::Begin => Token::Begin,
        NormalToken::Rescue => Token::Rescue,
        NormalToken::Ensure => Token::Ensure,
        NormalToken::Enum => Token::Enum,
        NormalToken::Class => Token::Class,
        NormalToken::SelfToken => Token::SelfToken,
        NormalToken::Property => Token::Property,
        NormalToken::Getter => Token::Getter,
        NormalToken::Setter => Token::Setter,
        NormalToken::Private => Token::Private,
        NormalToken::Return => Token::Return,
        NormalToken::Assert => Token::Assert,
        NormalToken::True => Token::True,
        NormalToken::False => Token::False,
        NormalToken::Nil => Token::Nil,
        NormalToken::Then => Token::Then,
        NormalToken::DotDot => Token::DotDot,
        NormalToken::Symbol(s) => Token::Symbol(s),
        NormalToken::Ident(s) => Token::Ident(s),
        NormalToken::Ivar(s) => Token::Ivar(s),
        NormalToken::Cvar(s) => Token::Cvar(s),
        NormalToken::Int(i) => Token::Int(i),
        NormalToken::Float(f) => Token::Float(f),
        NormalToken::Quote => unreachable!(), // Handled in loop
        NormalToken::Plus => Token::Plus,
        NormalToken::Minus => Token::Minus,
        NormalToken::Star => Token::Star,
        NormalToken::Slash => Token::Slash,
        NormalToken::Percent => Token::Percent,
        NormalToken::Assign => Token::Assign,
        NormalToken::Eq => Token::Eq,
        NormalToken::NotEq => Token::NotEq,
        NormalToken::Lt => Token::Lt,
        NormalToken::LtEq => Token::LtEq,
        NormalToken::Gt => Token::Gt,
        NormalToken::GtEq => Token::GtEq,
        NormalToken::And => Token::And,
        NormalToken::Or => Token::Or,
        NormalToken::Not => Token::Not,
        NormalToken::LParen => Token::LParen,
        NormalToken::RParen => Token::RParen,
        NormalToken::LBracket => Token::LBracket,
        NormalToken::RBracket => Token::RBracket,
        NormalToken::LBrace => Token::LBrace,
        NormalToken::RBrace => Token::RBrace,
        NormalToken::Comma => Token::Comma,
        NormalToken::Colon => Token::Colon,
        NormalToken::ColonColon => Token::ColonColon,
        NormalToken::Dot => Token::Dot,
        NormalToken::Pipe => Token::Pipe,
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Token::Def => write!(f, "def"),
            Token::Do => write!(f, "do"),
            Token::If => write!(f, "if"),
            Token::Elsif => write!(f, "elsif"),
            Token::Else => write!(f, "else"),
            Token::End => write!(f, "end"),
            Token::While => write!(f, "while"),
            Token::Until => write!(f, "until"),
            Token::For => write!(f, "for"),
            Token::In => write!(f, "in"),
            Token::Break => write!(f, "break"),
            Token::Next => write!(f, "next"),
            Token::Case => write!(f, "case"),
            Token::When => write!(f, "when"),
            Token::Begin => write!(f, "begin"),
            Token::Rescue => write!(f, "rescue"),
            Token::Ensure => write!(f, "ensure"),
            Token::Enum => write!(f, "enum"),
            Token::Class => write!(f, "class"),
            Token::SelfToken => write!(f, "self"),
            Token::Property => write!(f, "property"),
            Token::Getter => write!(f, "getter"),
            Token::Setter => write!(f, "setter"),
            Token::Private => write!(f, "private"),
            Token::Return => write!(f, "return"),
            Token::Assert => write!(f, "assert"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::Nil => write!(f, "nil"),
            Token::Then => write!(f, "then"),
            Token::DotDot => write!(f, ".."),
            Token::Symbol(s) => write!(f, ":{}", s),
            Token::Ident(s) => write!(f, "{}", s),
            Token::Ivar(s) => write!(f, "@{}", s),
            Token::Cvar(s) => write!(f, "@@{}", s),
            Token::Int(i) => write!(f, "{}", i),
            Token::Float(fl) => write!(f, "{}", fl),
            Token::String(s) => write!(f, "\"{}\"", s),
            Token::StringStart => write!(f, "\""),
            Token::StringEnd => write!(f, "\""),
            Token::StringText(s) => write!(f, "{}", s),
            Token::InterpolationStart => write!(f, "#{{"),
            Token::InterpolationEnd => write!(f, "}}"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Assign => write!(f, "="),
            Token::Eq => write!(f, "=="),
            Token::NotEq => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::LtEq => write!(f, "<="),
            Token::Gt => write!(f, ">"),
            Token::GtEq => write!(f, ">="),
            Token::And => write!(f, "&&"),
            Token::Or => write!(f, "||"),
            Token::Not => write!(f, "!"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::Comma => write!(f, ","),
            Token::Colon => write!(f, ":"),
            Token::ColonColon => write!(f, "::"),
            Token::Dot => write!(f, "."),
            Token::Pipe => write!(f, "|"),
        }
    }
}
