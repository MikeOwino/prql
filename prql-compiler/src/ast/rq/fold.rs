/// A trait to "fold" a PRQL AST (similar to a visitor), so we can transitively
/// apply some logic to a whole tree by just defining how we want to handle each
/// type.
use anyhow::Result;
use itertools::Itertools;

use crate::ast::pl::{InterpolateItem, SwitchCase};

use super::*;

// Fold pattern:
// - https://rust-unofficial.github.io/patterns/patterns/creational/fold.html
// Good discussions on the visitor / fold pattern:
// - https://github.com/rust-unofficial/patterns/discussions/236 (within this,
//   this comment looked interesting: https://github.com/rust-unofficial/patterns/discussions/236#discussioncomment-393517)
// - https://news.ycombinator.com/item?id=25620110

// For some functions, we want to call a default impl, because copying &
// pasting everything apart from a specific match is lots of repetition. So
// we define a function outside the trait, by default call it, and let
// implementors override the default while calling the function directly for
// some cases. Ref https://stackoverflow.com/a/66077767/3064736
pub trait RqFold {
    fn fold_transform(&mut self, transform: Transform) -> Result<Transform> {
        fold_transform(self, transform)
    }
    fn fold_transforms(&mut self, transforms: Vec<Transform>) -> Result<Vec<Transform>> {
        fold_transforms(self, transforms)
    }
    fn fold_table(&mut self, table: TableDecl) -> Result<TableDecl> {
        fold_table(self, table)
    }
    fn fold_relation(&mut self, relation: Relation) -> Result<Relation> {
        fold_relation(self, relation)
    }
    fn fold_relation_kind(&mut self, rel_kind: RelationKind) -> Result<RelationKind> {
        fold_relation_kind(self, rel_kind)
    }
    fn fold_table_ref(&mut self, table_ref: TableRef) -> Result<TableRef> {
        fold_table_ref(self, table_ref)
    }
    fn fold_query(&mut self, query: Query) -> Result<Query> {
        fold_query(self, query)
    }
    fn fold_expr(&mut self, mut expr: Expr) -> Result<Expr> {
        expr.kind = self.fold_expr_kind(expr.kind)?;
        Ok(expr)
    }
    fn fold_expr_kind(&mut self, kind: ExprKind) -> Result<ExprKind> {
        fold_expr_kind(self, kind)
    }
    fn fold_relation_column(&mut self, col: RelationColumn) -> Result<RelationColumn> {
        Ok(col)
    }
    fn fold_cid(&mut self, cid: CId) -> Result<CId> {
        Ok(cid)
    }
    fn fold_compute(&mut self, compute: Compute) -> Result<Compute> {
        fold_compute(self, compute)
    }
}

fn fold_compute<F: ?Sized + RqFold>(
    fold: &mut F,
    compute: Compute,
) -> Result<Compute, anyhow::Error> {
    Ok(Compute {
        id: fold.fold_cid(compute.id)?,
        expr: fold.fold_expr(compute.expr)?,
        window: compute.window.map(|w| fold_window(fold, w)).transpose()?,
        is_aggregation: compute.is_aggregation,
    })
}

fn fold_window<F: ?Sized + RqFold>(fold: &mut F, w: Window) -> Result<Window> {
    Ok(Window {
        frame: WindowFrame {
            kind: w.frame.kind,
            range: Range {
                start: w.frame.range.start.map(|x| fold.fold_expr(x)).transpose()?,
                end: w.frame.range.end.map(|x| fold.fold_expr(x)).transpose()?,
            },
        },
        partition: fold_cids(fold, w.partition)?,
        sort: fold_column_sorts(fold, w.sort)?,
    })
}

pub fn fold_table<F: ?Sized + RqFold>(fold: &mut F, t: TableDecl) -> Result<TableDecl> {
    Ok(TableDecl {
        id: t.id,
        name: t.name,
        relation: fold.fold_relation(t.relation)?,
    })
}

pub fn fold_relation<F: ?Sized + RqFold>(
    fold: &mut F,
    relation: Relation,
) -> Result<Relation, anyhow::Error> {
    Ok(Relation {
        kind: fold.fold_relation_kind(relation.kind)?,
        columns: relation.columns,
    })
}

pub fn fold_relation_kind<F: ?Sized + RqFold>(
    fold: &mut F,
    rel: RelationKind,
) -> Result<RelationKind> {
    Ok(match rel {
        RelationKind::ExternRef(table_ref) => RelationKind::ExternRef(table_ref),
        RelationKind::Pipeline(transforms) => {
            RelationKind::Pipeline(fold.fold_transforms(transforms)?)
        }
        RelationKind::Literal(lit) => RelationKind::Literal(lit),
        RelationKind::SString(items) => RelationKind::SString(fold_interpolate_items(fold, items)?),
    })
}

pub fn fold_table_ref<F: ?Sized + RqFold>(fold: &mut F, table_ref: TableRef) -> Result<TableRef> {
    Ok(TableRef {
        name: table_ref.name,
        source: table_ref.source,
        columns: table_ref
            .columns
            .into_iter()
            .map(|(col, cid)| -> Result<_> {
                Ok((fold.fold_relation_column(col)?, fold.fold_cid(cid)?))
            })
            .try_collect()?,
    })
}

pub fn fold_query<F: ?Sized + RqFold>(fold: &mut F, query: Query) -> Result<Query> {
    Ok(Query {
        def: query.def,
        relation: fold.fold_relation(query.relation)?,
        tables: query
            .tables
            .into_iter()
            .map(|t| fold.fold_table(t))
            .try_collect()?,
    })
}

fn fold_cids<F: ?Sized + RqFold>(fold: &mut F, cids: Vec<CId>) -> Result<Vec<CId>> {
    cids.into_iter().map(|i| fold.fold_cid(i)).try_collect()
}

pub fn fold_transforms<F: ?Sized + RqFold>(
    fold: &mut F,
    transforms: Vec<Transform>,
) -> Result<Vec<Transform>> {
    transforms
        .into_iter()
        .map(|t| fold.fold_transform(t))
        .try_collect()
}

pub fn fold_transform<T: ?Sized + RqFold>(
    fold: &mut T,
    mut transform: Transform,
) -> Result<Transform> {
    use Transform::*;

    transform = match transform {
        From(tid) => From(fold.fold_table_ref(tid)?),

        Compute(compute) => Compute(fold.fold_compute(compute)?),
        Aggregate { partition, compute } => Aggregate {
            partition: fold_cids(fold, partition)?,
            compute: fold_cids(fold, compute)?,
        },

        Select(ids) => Select(fold_cids(fold, ids)?),
        Filter(i) => Filter(fold.fold_expr(i)?),
        Sort(sorts) => Sort(fold_column_sorts(fold, sorts)?),
        Take(take) => Take(super::Take {
            partition: fold_cids(fold, take.partition)?,
            sort: fold_column_sorts(fold, take.sort)?,
            range: take.range,
        }),
        Join { side, with, filter } => Join {
            side,
            with: fold.fold_table_ref(with)?,
            filter: fold.fold_expr(filter)?,
        },
        Append(bottom) => Append(fold.fold_table_ref(bottom)?),
        Unique => Unique,
    };
    Ok(transform)
}

fn fold_column_sorts<T: ?Sized + RqFold>(
    fold: &mut T,
    sorts: Vec<ColumnSort<CId>>,
) -> Result<Vec<ColumnSort<CId>>> {
    sorts
        .into_iter()
        .map(|s| -> Result<ColumnSort<CId>> {
            Ok(ColumnSort {
                column: fold.fold_cid(s.column)?,
                direction: s.direction,
            })
        })
        .try_collect()
}

pub fn fold_expr_kind<F: ?Sized + RqFold>(fold: &mut F, kind: ExprKind) -> Result<ExprKind> {
    Ok(match kind {
        ExprKind::ColumnRef(cid) => ExprKind::ColumnRef(fold.fold_cid(cid)?),

        ExprKind::Binary { left, op, right } => ExprKind::Binary {
            left: Box::new(fold.fold_expr(*left)?),
            op,
            right: Box::new(fold.fold_expr(*right)?),
        },
        ExprKind::Unary { op, expr } => ExprKind::Unary {
            op,
            expr: Box::new(fold.fold_expr(*expr)?),
        },

        ExprKind::SString(items) => ExprKind::SString(fold_interpolate_items(fold, items)?),
        ExprKind::FString(items) => ExprKind::FString(fold_interpolate_items(fold, items)?),
        ExprKind::Switch(cases) => ExprKind::Switch(
            cases
                .into_iter()
                .map(|c| fold_switch_case(fold, c))
                .try_collect()?,
        ),
        ExprKind::BuiltInFunction { name, args } => ExprKind::BuiltInFunction {
            name,
            args: args.into_iter().map(|a| fold.fold_expr(a)).try_collect()?,
        },

        ExprKind::Literal(_) => kind,
    })
}

/// Helper
pub fn fold_optional_box<F: ?Sized + RqFold>(
    fold: &mut F,
    opt: Option<Box<Expr>>,
) -> Result<Option<Box<Expr>>> {
    Ok(match opt {
        Some(e) => Some(Box::new(fold.fold_expr(*e)?)),
        None => None,
    })
}

pub fn fold_interpolate_items<T: ?Sized + RqFold>(
    fold: &mut T,
    items: Vec<InterpolateItem<Expr>>,
) -> Result<Vec<InterpolateItem<Expr>>> {
    items
        .into_iter()
        .map(|i| fold_interpolate_item(fold, i))
        .try_collect()
}

pub fn fold_interpolate_item<T: ?Sized + RqFold>(
    fold: &mut T,
    item: InterpolateItem<Expr>,
) -> Result<InterpolateItem<Expr>> {
    Ok(match item {
        InterpolateItem::String(string) => InterpolateItem::String(string),
        InterpolateItem::Expr(expr) => InterpolateItem::Expr(Box::new(fold.fold_expr(*expr)?)),
    })
}

pub fn fold_switch_case<F: ?Sized + RqFold>(
    fold: &mut F,
    case: SwitchCase<Expr>,
) -> Result<SwitchCase<Expr>> {
    Ok(SwitchCase {
        condition: fold.fold_expr(case.condition)?,
        value: fold.fold_expr(case.value)?,
    })
}
