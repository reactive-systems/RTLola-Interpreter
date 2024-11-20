//! An attempt to implement dynamic dispatch codegen
//!
//! See [Building fast interpreters in Rust](https://blog.cloudflare.com/building-fast-interpreters-in-rust/)

use std::ops::{Add, BitAnd, BitOr, BitXor, Div, Mul, Neg, Not, Rem, Shl, Shr, Sub};
use std::rc::Rc;

use num::{FromPrimitive, ToPrimitive};
use ordered_float::NotNan;
use regex::bytes::Regex as BytesRegex;
use regex::Regex;
use rtlola_frontend::mir::{Constant, Expression, ExpressionKind, Offset, StreamAccessKind, Type};
use rust_decimal::{Decimal, MathematicalOps};
use string_template::Template;

use crate::evaluator::{ActivationConditionOp, EvaluationContext};
use crate::storage::Value;

pub(crate) trait Expr {
    fn compile(self) -> CompiledExpr;
}

#[derive(Clone)]
pub(crate) struct CompiledExpr(Rc<dyn Fn(&EvaluationContext<'_>) -> Value>);
// alternative: using Higher-Rank Trait Bounds (HRTBs)
// pub(crate) struct CompiledExpr<'s>(Box<dyn 's + for<'a> Fn(&EvaluationContext<'a>) -> Value>);

impl CompiledExpr {
    /// Creates a compiled expression IR from a generic closure.
    pub(crate) fn new(closure: impl 'static + Fn(&EvaluationContext<'_>) -> Value) -> Self {
        CompiledExpr(Rc::new(closure))
    }

    /// Creates a compiled expression returning the value of the `value_exp` if the `filter_exp` evaluates to true
    /// and None otherwise.
    pub(crate) fn create_filter(filter_exp: CompiledExpr, value_exp: CompiledExpr) -> Self {
        CompiledExpr::new(move |ctx| {
            if filter_exp.execute(ctx).as_bool() {
                value_exp.execute(ctx)
            } else {
                Value::None
            }
        })
    }

    /// Creates a compiled expression returning the value of the first clause that returned a value
    pub(crate) fn create_clauses(clauses: Vec<CompiledExpr>) -> Self {
        CompiledExpr::new(move |ctx| {
            clauses
                .iter()
                .map(|expr| expr.execute(ctx))
                .find(|value| !matches!(value, Value::None))
                .unwrap_or(Value::None)
        })
    }

    /// Creates a compiled expression checking whether the given activation condition is satisfied
    ///
    /// Is only used if the event-driven pacing of a single eval clause is different than the pacing of the
    /// whole output.
    pub(crate) fn create_activation(expr: CompiledExpr, ac: ActivationConditionOp) -> Self {
        CompiledExpr::new(move |ctx| {
            if ctx.is_active(&ac) {
                expr.execute(ctx)
            } else {
                Value::None
            }
        })
    }

    /// Executes a filter against a provided context with values.
    pub(crate) fn execute(&self, ctx: &EvaluationContext) -> Value {
        self.0(ctx)
    }
}

impl Expr for Expression {
    fn compile(self) -> CompiledExpr {
        use ExpressionKind::*;
        match self.kind {
            LoadConstant(c) => {
                let v = match c {
                    Constant::Bool(b) => Value::Bool(b),
                    Constant::UInt(u) => Value::Unsigned(u),
                    Constant::Int(i) => Value::Signed(i),
                    Constant::Float(f) => Value::Float(NotNan::new(f).expect("Constants shouldn't allow NaN")),
                    Constant::Str(s) => Value::Str(s.into_boxed_str()),
                    Constant::Decimal(i) => Value::Decimal(i),
                };
                CompiledExpr::new(move |_| v.clone())
            },
            ParameterAccess(_target, idx) => CompiledExpr::new(move |ctx| ctx.parameter[idx].clone()),
            ArithLog(op, operands) => {
                let f_operands: Vec<CompiledExpr> = operands.into_iter().map(|e| e.compile()).collect();

                macro_rules! create_unop {
                    ($fn:ident) => {
                        CompiledExpr::new(move |ctx| {
                            let lhs = f_operands[0].execute(ctx);
                            lhs.$fn()
                        })
                    };
                }
                macro_rules! create_binop {
                    ($fn:ident) => {
                        CompiledExpr::new(move |ctx| {
                            let lhs = f_operands[0].execute(ctx);
                            let rhs = f_operands[1].execute(ctx);
                            lhs.$fn(rhs)
                        })
                    };
                }
                macro_rules! create_cmp {
                    ($fn:ident) => {
                        CompiledExpr::new(move |ctx| {
                            let lhs = f_operands[0].execute(ctx);
                            let rhs = f_operands[1].execute(ctx);
                            Value::Bool(lhs.$fn(&rhs))
                        })
                    };
                }
                macro_rules! create_lazyop {
                    ($b:expr) => {
                        CompiledExpr::new(move |ctx| {
                            let lhs = f_operands[0].execute(ctx).as_bool();
                            if lhs == $b {
                                Value::Bool($b)
                            } else {
                                let res = f_operands[1].execute(ctx);
                                assert!(res.is_bool());
                                res
                            }
                        })
                    };
                }

                use rtlola_frontend::mir::ArithLogOp::*;
                match op {
                    Not => create_unop!(not),
                    BitNot => create_unop!(not),
                    Neg => create_unop!(neg),
                    Add => create_binop!(add),
                    Sub => create_binop!(sub),
                    Mul => create_binop!(mul),
                    Div => create_binop!(div),
                    Rem => create_binop!(rem),
                    Pow => create_binop!(pow),
                    Eq => create_cmp!(eq),
                    Lt => create_cmp!(lt),
                    Le => create_cmp!(le),
                    Ne => create_cmp!(ne),
                    Ge => create_cmp!(ge),
                    Gt => create_cmp!(gt),
                    And => create_lazyop!(false),
                    Or => create_lazyop!(true),
                    BitAnd => create_binop!(bitand),
                    BitOr => create_binop!(bitor),
                    BitXor => create_binop!(bitxor),
                    Shl => create_binop!(shl),
                    Shr => create_binop!(shr),
                }
            },

            StreamAccess {
                target,
                parameters,
                access_kind,
            } => {
                let paras: Vec<CompiledExpr> = parameters.into_iter().map(|e| e.compile()).collect();
                macro_rules! create_access {
                    ($fn:ident, $target:ident $( , $arg:ident )*) => {
                        CompiledExpr::new(move |ctx| {
                            let parameter: Vec<Value> = paras.iter().map(|p| p.execute(ctx)).collect();
                            ctx.$fn($target, parameter.as_slice(), $($arg),*)
                        })
                    };
                }
                match access_kind {
                    StreamAccessKind::Sync => create_access!(lookup_latest_check, target),
                    StreamAccessKind::DiscreteWindow(wref) | StreamAccessKind::SlidingWindow(wref) => {
                        create_access!(lookup_window, wref)
                    },
                    StreamAccessKind::Hold => create_access!(lookup_latest, target),
                    StreamAccessKind::Offset(offset) => {
                        let offset = match offset {
                            Offset::Future(_) => unimplemented!(),
                            Offset::Past(u) => -(u as i16),
                        };
                        create_access!(lookup_with_offset, target, offset)
                    },
                    StreamAccessKind::Get => create_access!(lookup_current, target),
                    StreamAccessKind::Fresh => create_access!(lookup_fresh, target),
                    StreamAccessKind::InstanceAggregation(wref) => {
                        CompiledExpr::new(move |ctx| ctx.lookup_instance_aggr(wref))
                    },
                }
            },

            Ite {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let f_condition = condition.compile();
                let f_consequence = consequence.compile();
                let f_alternative = alternative.compile();

                CompiledExpr::new(move |ctx| {
                    let cond = f_condition.execute(ctx).as_bool();
                    if cond {
                        f_consequence.execute(ctx)
                    } else {
                        f_alternative.execute(ctx)
                    }
                })
            },

            Tuple(entries) => {
                let f_entries: Vec<CompiledExpr> = entries.into_iter().map(|e| e.compile()).collect();
                CompiledExpr::new(move |ctx| Value::Tuple(f_entries.iter().map(|f| f.execute(ctx)).collect()))
            },

            Function(name, args) => {
                assert!(!args.is_empty());
                let f_arg = args[0].clone().compile();

                macro_rules! create_decimalfn {
                    ($fn:ident) => {
                        CompiledExpr::new(move |ctx| {
                            let arg = f_arg.execute(ctx);
                            match arg {
                                Value::Float(f) => Value::try_from(f.$fn()).unwrap(),
                                Value::Decimal(f) => Value::try_from(f.$fn()).unwrap(),
                                _ => unreachable!(),
                            }
                        })
                    };
                }

                macro_rules! create_floatfn {
                    ($fn:ident) => {
                        CompiledExpr::new(move |ctx| {
                            let arg = f_arg.execute(ctx);
                            match arg {
                                Value::Float(f) => Value::try_from(f.$fn()).unwrap(),
                                _ => unreachable!(),
                            }
                        })
                    };
                }

                macro_rules! create_binary_arith {
                    ($fn:ident) => {{
                        if args.len() != 2 {
                            unreachable!("wrong number of arguments for function $fn")
                        }
                        CompiledExpr::new(move |ctx| {
                            let fst = f_arg.execute(ctx);
                            let snd = args[1].clone().compile().execute(ctx);
                            match (fst, snd) {
                                (Value::Float(f1), Value::Float(f2)) => Value::Float(f1.$fn(f2)),
                                (Value::Signed(s1), Value::Signed(s2)) => Value::Signed(s1.$fn(s2)),
                                (Value::Unsigned(u1), Value::Unsigned(u2)) => Value::Unsigned(u1.$fn(u2)),
                                (v1, v2) => unreachable!("wrong Value types of {:?}, {:?} for function $fn", v1, v2),
                            }
                        })
                    }};
                }

                match name.as_ref() {
                    "sqrt" => create_decimalfn!(sqrt),
                    "sin" => create_decimalfn!(sin),
                    "cos" => create_decimalfn!(cos),
                    "tan" => create_decimalfn!(tan),
                    "arcsin" => create_floatfn!(asin),
                    "arccos" => create_floatfn!(acos),
                    "arctan" => create_floatfn!(atan),
                    "abs" => {
                        CompiledExpr::new(move |ctx| {
                            let arg = f_arg.execute(ctx);
                            match arg {
                                Value::Float(f) => Value::try_from(f.abs()).unwrap(),
                                Value::Signed(i) => Value::Signed(i.abs()),
                                v => unreachable!("wrong Value type of {:?}, for function abs", v),
                            }
                        })
                    },
                    "min" => create_binary_arith!(min),
                    "max" => create_binary_arith!(max),
                    "matches" => {
                        assert!(args.len() >= 2);
                        let is_bytes = args[0].ty == Type::Bytes;
                        let re_str = match &args[1].kind {
                            LoadConstant(Constant::Str(s)) => s,
                            _ => unreachable!("regex should be a string literal"),
                        };
                        if !is_bytes {
                            let re = Regex::new(re_str).expect("Given regular expression was invalid");
                            CompiledExpr::new(move |ctx| {
                                let val = f_arg.execute(ctx);
                                if let Value::Str(s) = &val {
                                    Value::Bool(re.is_match(s))
                                } else {
                                    unreachable!("expected `String`, found {:?}", val);
                                }
                            })
                        } else {
                            let re = BytesRegex::new(re_str).expect("Given regular expression was invalid");
                            CompiledExpr::new(move |ctx| {
                                let val = f_arg.execute(ctx);
                                if let Value::Bytes(b) = &val {
                                    Value::Bool(re.is_match(b))
                                } else {
                                    unreachable!("expected `Bytes`, found {:?}", val);
                                }
                            })
                        }
                    },
                    "at" => {
                        assert_eq!(args.len(), 2);
                        let index_arg = args[1].clone().compile();
                        CompiledExpr::new(move |ctx| {
                            let val = f_arg.execute(ctx);
                            let index = index_arg.execute(ctx);
                            match (val, index) {
                                (Value::Bytes(b), Value::Unsigned(idx)) => {
                                    if let Some(&byte) = b.get(idx as usize) {
                                        Value::Unsigned(byte.into())
                                    } else {
                                        Value::None
                                    }
                                },
                                (val, _) => unreachable!("expected `Bytes`, found {:?}", val),
                            }
                        })
                    },
                    "format" => {
                        assert!(args.len() > 1);
                        let LoadConstant(Constant::Str(fstr)) = &args[0].kind else {
                            panic!("format string expected to be static");
                        };
                        let template = Template::new(fstr);
                        let args: Vec<_> = args.into_iter().skip(1).map(Expression::compile).collect();
                        CompiledExpr::new(move |ctx| {
                            let vals = args.iter().map(|exp| exp.execute(ctx).to_string()).collect::<Vec<_>>();
                            let vals_ref = vals.iter().map(|s| s.as_str()).collect::<Vec<_>>();
                            template.render_positional(&vals_ref).into()
                        })
                    },
                    "round" => {
                        assert!(args.len() > 1);
                        let LoadConstant(Constant::UInt(points)) = &args[1].kind else {
                            panic!("decimal points expected to be static");
                        };
                        let decimals = 10u64.pow(*points as u32) as f64;
                        CompiledExpr::new(move |ctx| {
                            let arg = f_arg.execute(ctx);
                            match arg {
                                Value::Float(f) => Value::try_from((f * decimals).round() / decimals).unwrap(),
                                _ => unreachable!(),
                            }
                        })
                    },
                    f => unreachable!("Unknown function: {}, args: {:?}", f, args),
                }
            },

            Convert { expr: f_expr } => {
                let from_ty = &f_expr.ty.clone();
                let to_ty = &self.ty;
                let f_expr = f_expr.compile();
                macro_rules! create_convert {
                    (Float, $to:ident, $ty:ty) => {
                        CompiledExpr::new(move |ctx| {
                            let v = f_expr.execute(ctx);
                            match v {
                                Value::Float(f) => Value::$to(f.into_inner() as $ty),
                                v => {
                                    unreachable!(
                                        "Value type of {:?} does not match convert from type {:?}",
                                        v,
                                        Value::try_from(0.0).unwrap()
                                    )
                                },
                            }
                        })
                    };
                    ($from:ident, Float, $ty:ty) => {
                        CompiledExpr::new(move |ctx| {
                            let v = f_expr.execute(ctx);
                            match v {
                                Value::$from(v) => Value::try_from(v as $ty).unwrap(),
                                v => {
                                    unreachable!(
                                        "Value type of {:?} does not match convert from type {:?}",
                                        v,
                                        Value::$from(0)
                                    )
                                },
                            }
                        })
                    };
                    ($from:ident, $to:ident, $ty:ty) => {
                        CompiledExpr::new(move |ctx| {
                            let v = f_expr.execute(ctx);
                            match v {
                                Value::$from(v) => Value::$to(v as $ty),
                                v => {
                                    unreachable!(
                                        "Value type of {:?} does not match convert from type {:?}",
                                        v,
                                        Value::$from(0)
                                    )
                                },
                            }
                        })
                    };
                    ($from:ident, $to:ident, $fn:expr) => {
                        CompiledExpr::new(move |ctx| {
                            let v = f_expr.execute(ctx);
                            match v {
                                Value::$from(v) => Value::$to($fn(v)),
                                v => {
                                    unreachable!(
                                        "Value type of {:?} does not match convert from type {:?}",
                                        v,
                                        stringify!($from)
                                    )
                                },
                            }
                        })
                    };
                }

                use Type::*;
                match (from_ty, to_ty) {
                    (UInt(_), UInt(_)) => f_expr,
                    (UInt(_), Int(_)) => create_convert!(Unsigned, Signed, i64),
                    (UInt(_), Float(_)) => create_convert!(Unsigned, Float, f64),
                    (Int(_), UInt(_)) => create_convert!(Signed, Unsigned, u64),
                    (Int(_), Int(_)) => f_expr,
                    (Int(_), Float(_)) => create_convert!(Signed, Float, f64),
                    (Float(_), UInt(_)) => create_convert!(Float, Unsigned, u64),
                    (Float(_), Int(_)) => create_convert!(Float, Signed, i64),
                    (Fixed(_), Fixed(_)) => f_expr,
                    (UFixed(_), UFixed(_)) => f_expr,
                    (UInt(_), Fixed(_) | UFixed(_)) => create_convert!(Signed, Decimal, |v: i64| Decimal::from(v)),
                    (Int(_), Fixed(_) | UFixed(_)) => create_convert!(Unsigned, Decimal, |v: u64| Decimal::from(v)),
                    (Float(_), Fixed(_) | UFixed(_)) => {
                        create_convert!(Float, Decimal, |v: NotNan<f64>| {
                            Decimal::from_f64(v.to_f64().unwrap()).unwrap()
                        })
                    },
                    (Fixed(_) | UFixed(_), Float(_)) => {
                        create_convert!(Decimal, Float, |v: Decimal| {
                            NotNan::try_from(v.to_f64().unwrap()).unwrap()
                        })
                    },
                    (Fixed(_) | UFixed(_), Int(_)) => {
                        create_convert!(Decimal, Signed, |v: Decimal| { v.round().to_i64().unwrap() })
                    },
                    (Fixed(_) | UFixed(_), UInt(_)) => {
                        create_convert!(Decimal, Unsigned, |v: Decimal| { v.round().to_u64().unwrap() })
                    },
                    (from, to) => unreachable!("from: {:?}, to: {:?}", from, to),
                }
            },

            Default { expr, default, .. } => {
                let f_expr = expr.compile();
                let f_default = default.compile();
                CompiledExpr::new(move |ctx| {
                    let v = f_expr.execute(ctx);
                    if let Value::None = v {
                        f_default.execute(ctx)
                    } else {
                        v
                    }
                })
            },

            TupleAccess(expr, num) => {
                let f_expr = expr.compile();
                CompiledExpr::new(move |ctx| {
                    let inner = f_expr.execute(ctx);
                    match inner {
                        Value::Tuple(elements) => elements[num].clone(),
                        Value::None => Value::None,
                        _ => unreachable!("verified by type checker"),
                    }
                })
            },
        }
    }
}
