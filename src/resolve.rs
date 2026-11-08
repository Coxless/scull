//! 名前解決:トップレベルの名前表を作り、宣言側の名前と型を検査する(仕様 §5.1)。

use crate::ast::*;
use crate::diag::{Diagnostic, Span};
use std::collections::HashMap;

pub const BUILTIN_NAMES: &[&str] = &["Int", "Bool", "String", "Console"];

#[derive(Clone, Debug)]
pub struct OpSig {
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Ty,
}

#[derive(Clone, Debug)]
pub struct FunSig {
    pub params: Vec<Ty>,
    pub ret: Ty,
    /// 有効な能力だけを辞書順に並べたもの。
    pub uses: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct EffectInfo {
    pub ops: Vec<OpSig>,
}

#[derive(Clone, Debug)]
pub struct HandlerInfo {
    /// 対象のエフェクト(存在しなければ None)。
    pub effect: Option<String>,
    pub uses: Vec<String>,
    /// 各操作の実装のシグネチャ(宣言順)。
    pub ops: Vec<OpSig>,
}

#[derive(Debug, Default)]
pub struct Globals {
    /// `prog.funs` と同じ順序。
    pub funs: Vec<FunSig>,
    pub fun_index: HashMap<String, usize>,
    /// ユーザー定義と組み込み(Console)のエフェクト。
    pub effects: HashMap<String, EffectInfo>,
    /// `prog.handlers` と同じ順序。
    pub handlers: Vec<HandlerInfo>,
    pub handler_index: HashMap<String, usize>,
}

impl Globals {
    pub fn fun(&self, name: &str) -> Option<&FunSig> {
        self.fun_index.get(name).map(|&i| &self.funs[i])
    }

    pub fn handler(&self, name: &str) -> Option<&HandlerInfo> {
        self.handler_index.get(name).map(|&i| &self.handlers[i])
    }
}

pub fn is_upper(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

pub fn is_lower(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
}

/// 小文字で始まるべき名前を検査する(E0105)。
pub fn check_lower(id: &Ident, what: &str, diags: &mut Vec<Diagnostic>) -> bool {
    if is_lower(&id.name) {
        return true;
    }
    diags.push(Diagnostic::error(
        "E0105",
        id.span,
        format!("{what}は小文字または `_` で始めます(`{}`)", id.name),
    ));
    false
}

/// 大文字で始まるべき名前を検査する(E0105)。
pub fn check_upper(id: &Ident, what: &str, diags: &mut Vec<Diagnostic>) -> bool {
    if is_upper(&id.name) {
        return true;
    }
    diags.push(Diagnostic::error(
        "E0105",
        id.span,
        format!("{what}は大文字で始めます(`{}`)", id.name),
    ));
    false
}

pub fn resolve_type(t: &TypeAnn, diags: &mut Vec<Diagnostic>) -> Ty {
    match t.name.as_str() {
        "Int" => Ty::Int,
        "Bool" => Ty::Bool,
        "String" => Ty::Str,
        name if !is_upper(name) => {
            diags.push(Diagnostic::error(
                "E0105",
                t.span,
                format!("型名は大文字で始めます(`{name}`)"),
            ));
            Ty::Error
        }
        name => {
            diags.push(
                Diagnostic::error("E0101", t.span, format!("型 `{name}` は定義されていません"))
                    .with_help("Scull 0 の型は `Int`、`Bool`、`String` だけです"),
            );
            Ty::Error
        }
    }
}

fn ret_type(t: &Option<TypeAnn>, diags: &mut Vec<Diagnostic>) -> Ty {
    match t {
        None => Ty::Unit,
        Some(t) => resolve_type(t, diags),
    }
}

/// 引数の型を解決し、引数名の検査(小文字・重複)も行う。
fn params(ps: &[Param], diags: &mut Vec<Diagnostic>) -> Vec<Ty> {
    let mut seen: Vec<&str> = Vec::new();
    let mut tys = Vec::new();
    for p in ps {
        check_lower(&p.name, "引数名", diags);
        if seen.contains(&p.name.name.as_str()) {
            diags.push(Diagnostic::error(
                "E0102",
                p.name.span,
                format!("引数 `{}` が重複しています", p.name.name),
            ));
        }
        seen.push(&p.name.name);
        tys.push(resolve_type(&p.ty, diags));
    }
    tys
}

pub struct Resolver<'a> {
    g: Globals,
    diags: &'a mut Vec<Diagnostic>,
    declared: HashMap<String, Span>,
}

pub fn resolve(prog: &Program, diags: &mut Vec<Diagnostic>) -> Globals {
    let mut r = Resolver {
        g: Globals::default(),
        diags,
        declared: HashMap::new(),
    };
    r.g.effects.insert(
        "Console".to_string(),
        EffectInfo {
            ops: vec![OpSig {
                name: "print".to_string(),
                params: vec![Ty::Str],
                ret: Ty::Unit,
            }],
        },
    );
    r.run(prog);
    r.g
}

impl<'a> Resolver<'a> {
    /// トップレベルの名前を登録する。重複や組み込み名の再定義なら false。
    fn declare(&mut self, id: &Ident) -> bool {
        if BUILTIN_NAMES.contains(&id.name.as_str()) {
            self.diags.push(Diagnostic::error(
                "E0102",
                id.span,
                format!("`{}` は組み込みの名前なので再定義できません", id.name),
            ));
            return false;
        }
        if self.declared.contains_key(&id.name) {
            self.diags.push(Diagnostic::error(
                "E0102",
                id.span,
                format!("`{}` はすでに定義されています", id.name),
            ));
            return false;
        }
        self.declared.insert(id.name.clone(), id.span);
        true
    }

    /// `uses` 節を解決する。有効なエフェクトだけを辞書順で返す。
    fn uses(&mut self, uses: &[Ident], prog: &Program) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for u in uses {
            if !check_upper(u, "能力(エフェクト)の名前", self.diags) {
                continue;
            }
            if out.contains(&u.name) {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    u.span,
                    format!("能力 `{}` が `uses` に重複して書かれています", u.name),
                ));
                continue;
            }
            if self.g.effects.contains_key(&u.name) {
                out.push(u.name.clone());
            } else {
                self.unknown_effect(u, prog);
            }
        }
        out.sort();
        out
    }

    fn unknown_effect(&mut self, id: &Ident, prog: &Program) {
        let d = if prog.handlers.iter().any(|h| h.name.name == id.name) {
            Diagnostic::error(
                "E0302",
                id.span,
                format!("`{}` はハンドラであり、エフェクトではありません", id.name),
            )
        } else {
            Diagnostic::error(
                "E0302",
                id.span,
                format!("エフェクト `{}` は存在しません", id.name),
            )
        };
        self.diags.push(d);
    }

    fn run(&mut self, prog: &Program) {
        // 1. エフェクト
        for e in &prog.effects {
            check_upper(&e.name, "エフェクト名", self.diags);
            let ok = self.declare(&e.name);
            let mut ops: Vec<OpSig> = Vec::new();
            for op in &e.ops {
                check_lower(&op.name, "操作名", self.diags);
                if ops.iter().any(|o| o.name == op.name.name) {
                    self.diags.push(Diagnostic::error(
                        "E0102",
                        op.name.span,
                        format!("操作 `{}` が重複しています", op.name.name),
                    ));
                    continue;
                }
                let params = params(&op.params, self.diags);
                let ret = ret_type(&op.ret, self.diags);
                ops.push(OpSig {
                    name: op.name.name.clone(),
                    params,
                    ret,
                });
            }
            if ok {
                self.g
                    .effects
                    .insert(e.name.name.clone(), EffectInfo { ops });
            }
        }

        // 2. ハンドラ
        for h in &prog.handlers {
            check_upper(&h.name, "ハンドラ名", self.diags);
            let ok = self.declare(&h.name);
            let effect = if check_upper(&h.effect, "エフェクト名", self.diags) {
                if self.g.effects.contains_key(&h.effect.name) {
                    Some(h.effect.name.clone())
                } else {
                    self.unknown_effect(&h.effect, prog);
                    None
                }
            } else {
                None
            };
            let uses = self.uses(&h.uses, prog);
            let mut ops = Vec::new();
            for f in &h.ops {
                check_lower(&f.name, "操作名", self.diags);
                let params = params(&f.params, self.diags);
                let ret = ret_type(&f.ret, self.diags);
                ops.push(OpSig {
                    name: f.name.name.clone(),
                    params,
                    ret,
                });
            }
            let idx = self.g.handlers.len();
            self.g.handlers.push(HandlerInfo { effect, uses, ops });
            if ok {
                self.g.handler_index.insert(h.name.name.clone(), idx);
            }
        }

        // 3. 関数
        for f in &prog.funs {
            check_lower(&f.name, "関数名", self.diags);
            let ok = self.declare(&f.name);
            let params = params(&f.params, self.diags);
            let ret = ret_type(&f.ret, self.diags);
            let uses = self.uses(&f.uses, prog);
            let idx = self.g.funs.len();
            self.g.funs.push(FunSig { params, ret, uses });
            if ok {
                self.g.fun_index.insert(f.name.name.clone(), idx);
            }
        }
    }
}
