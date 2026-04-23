use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\n\f]+")] // Skip whitespace
#[logos(skip(r"#[^\n]*", allow_greedy = true))] // Skip comments
pub enum Token {
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
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("nil")]
    Nil,

    #[regex("[a-zA-Z_][a-zA-Z0-9_]*\\??", |lex| lex.slice().to_string())]
    Ident(String),

    #[regex("@[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Ivar(String),

    #[regex("@@[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[2..].to_string())]
    Cvar(String),

    #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    Int(i64),

    #[regex(r"[0-9]+\.[0-9]+", |lex| lex.slice().parse::<f64>().ok())]
    Float(f64),

    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        let content = &s[1..s.len()-1];
        Some(content.replace("\\\"", "\"").replace("\\n", "\n").replace("\\t", "\t"))
    })]
    String(String),

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
    #[token(".")]
    Dot,
    #[token("|")]
    Pipe,
}
