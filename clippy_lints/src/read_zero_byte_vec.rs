use clippy_utils::{
    diagnostics::{span_lint, span_lint_and_sugg},
    higher::{get_vec_init_kind, VecInitKind},
    source::snippet,
    visitors::expr_visitor_no_bodies,
};
use hir::{intravisit::Visitor, ExprKind, Local, PatKind, PathSegment, QPath, StmtKind};
use rustc_errors::Applicability;
use rustc_hir as hir;
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint_pass, declare_tool_lint};

declare_clippy_lint! {
    /// ### What it does
    /// This lint catches reads into a zero-length `Vec`.
    /// Especially in the case of a call to `with_capacity`, this lint warns that read
    /// gets the number of bytes from the `Vec`'s length, not its capacity.
    ///
    /// ### Why is this bad?
    /// Reading zero bytes is almost certainly not the intended behavior.
    ///
    /// ### Known problems
    /// In theory, a very unusual read implementation could assign some semantic meaning
    /// to zero-byte reads. But it seems exceptionally unlikely that code intending to do
    /// a zero-byte read would allocate a `Vec` for it.
    ///
    /// ### Example
    /// ```rust
    /// use std::io;
    /// fn foo<F: io::Read>(mut f: F) {
    ///     let mut data = Vec::with_capacity(100);
    ///     f.read(&mut data).unwrap();
    /// }
    /// ```
    /// Use instead:
    /// ```rust
    /// use std::io;
    /// fn foo<F: io::Read>(mut f: F) {
    ///     let mut data = Vec::with_capacity(100);
    ///     data.resize(100, 0);
    ///     f.read(&mut data).unwrap();
    /// }
    /// ```
    #[clippy::version = "1.63.0"]
    pub READ_ZERO_BYTE_VEC,
    correctness,
    "checks for reads into a zero-length `Vec`"
}
declare_lint_pass!(ReadZeroByteVec => [READ_ZERO_BYTE_VEC]);

impl<'tcx> LateLintPass<'tcx> for ReadZeroByteVec {
    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &hir::Block<'tcx>) {
        for (idx, stmt) in block.stmts.iter().enumerate() {
            if !stmt.span.from_expansion()
                // matches `let v = Vec::new();`
                && let StmtKind::Local(local) = stmt.kind
                && let Local { pat, init: Some(init), .. } = local
                && let PatKind::Binding(_, _, ident, _) = pat.kind
                && let Some(vec_init_kind) = get_vec_init_kind(cx, init)
            {
                // finds use of `_.read(&mut v)`
                let mut read_found = false;
                let mut visitor = expr_visitor_no_bodies(|expr| {
                    if let ExprKind::MethodCall(path, _self, [arg], _) = expr.kind
                        && let PathSegment { ident: read_or_read_exact, .. } = *path
                        && matches!(read_or_read_exact.as_str(), "read" | "read_exact")
                        && let ExprKind::AddrOf(_, hir::Mutability::Mut, inner) = arg.kind
                        && let ExprKind::Path(QPath::Resolved(None, inner_path)) = inner.kind
                        && let [inner_seg] = inner_path.segments
                        && ident.name == inner_seg.ident.name
                    {
                        read_found = true;
                    }
                    !read_found
                });

                let next_stmt_span;
                if idx == block.stmts.len() - 1 {
                    // case { .. stmt; expr }
                    if let Some(e) = block.expr {
                        visitor.visit_expr(e);
                        next_stmt_span = e.span;
                    } else {
                        return;
                    }
                } else {
                    // case { .. stmt; stmt; .. }
                    let next_stmt = &block.stmts[idx + 1];
                    visitor.visit_stmt(next_stmt);
                    next_stmt_span = next_stmt.span;
                }
                drop(visitor);

                if read_found && !next_stmt_span.from_expansion() {
                    let applicability = Applicability::MaybeIncorrect;
                    match vec_init_kind {
                        VecInitKind::WithConstCapacity(len) => {
                            span_lint_and_sugg(
                                cx,
                                READ_ZERO_BYTE_VEC,
                                next_stmt_span,
                                "reading zero byte data to `Vec`",
                                "try",
                                format!("{}.resize({len}, 0); {}",
                                    ident.as_str(),
                                    snippet(cx, next_stmt_span, "..")
                                ),
                                applicability,
                            );
                        }
                        VecInitKind::WithExprCapacity(hir_id) => {
                            let e = cx.tcx.hir().expect_expr(hir_id);
                            span_lint_and_sugg(
                                cx,
                                READ_ZERO_BYTE_VEC,
                                next_stmt_span,
                                "reading zero byte data to `Vec`",
                                "try",
                                format!("{}.resize({}, 0); {}",
                                    ident.as_str(),
                                    snippet(cx, e.span, ".."),
                                    snippet(cx, next_stmt_span, "..")
                                ),
                                applicability,
                            );
                        }
                        _ => {
                            span_lint(
                                cx,
                                READ_ZERO_BYTE_VEC,
                                next_stmt_span,
                                "reading zero byte data to `Vec`",
                            );

                        }
                    }
                }
            }
        }
    }
}
