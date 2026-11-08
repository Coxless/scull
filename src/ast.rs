//! 抽象構文木。

use crate::diag::Span;
use std::cell::Cell;

/// Scull 0 の型。`Error` は検査エラー後の連鎖的な報告を抑えるための内部用の型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
    Str,
    Unit,
    Error,
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "Int",
            Ty::Bool => "Bool",
            Ty::Str => "String",
            Ty::Unit => "Unit",
            Ty::Error => "{エラー}",
        }
    }

    /// 型が一致するか(`Error` は何とでも一致する)。
    pub fn compatible(self, other: Ty) -> bool {
        self == other || self == Ty::Error || other == Ty::Error
    }
}

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeAnn {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeAnn,
}

#[derive(Debug, Default)]
pub struct Program {
    pub funs: Vec<FunDecl>,
    pub effects: Vec<EffectDecl>,
    pub handlers: Vec<HandlerDecl>,
}

#[derive(Debug)]
pub struct FunDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<TypeAnn>,
    pub uses: Vec<Ident>,
    pub body: Block,
}

#[derive(Debug)]
pub struct OpSigDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<TypeAnn>,
}

#[derive(Debug)]
pub struct EffectDecl {
    pub name: Ident,
    pub ops: Vec<OpSigDecl>,
}

#[derive(Debug)]
pub struct HandlerDecl {
    pub name: Ident,
    pub effect: Ident,
    pub uses: Vec<Ident>,
    /// 各操作の実装。`uses` は常に空。
    pub ops: Vec<FunDecl>,
}

#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

#[derive(Debug)]
pub enum Stmt {
    Let {
        mutable: bool,
        name: Ident,
        ann: Option<TypeAnn>,
        init: Expr,
    },
    Assign {
        name: Ident,
        op: AssignOp,
        value: Expr,
        op_span: Span,
    },
    If {
        cond: Expr,
        then: Block,
        els: Option<Else>,
    },
    While {
        cond: Expr,
        body: Block,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Handle {
        effect: Ident,
        handler: Ident,
        body: Block,
    },
    Expr(Expr),
}

#[derive(Debug)]
pub enum Else {
    Block(Block),
    If(Box<Stmt>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// 型検査の結果(コード生成で使う)。
    pub ty: Cell<Ty>,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Expr {
        Expr {
            kind,
            span,
            ty: Cell::new(Ty::Error),
        }
    }
}

#[derive(Debug)]
pub enum StrPiece {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Str(Vec<StrPiece>),
    Var(Ident),
    Call {
        callee: Ident,
        args: Vec<Expr>,
    },
    OpCall {
        effect: Ident,
        op: Ident,
        args: Vec<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}
