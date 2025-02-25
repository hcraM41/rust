//! Module that implements what will become the rustc side of Stable MIR.
//!
//! This module is responsible for building Stable MIR components from internal components.
//!
//! This module is not intended to be invoked directly by users. It will eventually
//! become the public API of rustc that will be invoked by the `stable_mir` crate.
//!
//! For now, we are developing everything inside `rustc`, thus, we keep this module private.

use crate::rustc_internal::{self, opaque};
use crate::stable_mir::ty::{FloatTy, IntTy, Movability, RigidTy, TyKind, UintTy};
use crate::stable_mir::{self, Context};
use rustc_hir as hir;
use rustc_middle::mir;
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::def_id::{CrateNum, DefId, LOCAL_CRATE};
use rustc_target::abi::FieldIdx;
use tracing::debug;

impl<'tcx> Context for Tables<'tcx> {
    fn local_crate(&self) -> stable_mir::Crate {
        smir_crate(self.tcx, LOCAL_CRATE)
    }

    fn external_crates(&self) -> Vec<stable_mir::Crate> {
        self.tcx.crates(()).iter().map(|crate_num| smir_crate(self.tcx, *crate_num)).collect()
    }

    fn find_crate(&self, name: &str) -> Option<stable_mir::Crate> {
        [LOCAL_CRATE].iter().chain(self.tcx.crates(()).iter()).find_map(|crate_num| {
            let crate_name = self.tcx.crate_name(*crate_num).to_string();
            (name == crate_name).then(|| smir_crate(self.tcx, *crate_num))
        })
    }

    fn all_local_items(&mut self) -> stable_mir::CrateItems {
        self.tcx.mir_keys(()).iter().map(|item| self.crate_item(item.to_def_id())).collect()
    }
    fn entry_fn(&mut self) -> Option<stable_mir::CrateItem> {
        Some(self.crate_item(self.tcx.entry_fn(())?.0))
    }
    fn mir_body(&mut self, item: &stable_mir::CrateItem) -> stable_mir::mir::Body {
        let def_id = self.item_def_id(item);
        let mir = self.tcx.optimized_mir(def_id);
        stable_mir::mir::Body {
            blocks: mir
                .basic_blocks
                .iter()
                .map(|block| stable_mir::mir::BasicBlock {
                    terminator: block.terminator().stable(self),
                    statements: block
                        .statements
                        .iter()
                        .map(|statement| statement.stable(self))
                        .collect(),
                })
                .collect(),
            locals: mir.local_decls.iter().map(|decl| self.intern_ty(decl.ty)).collect(),
        }
    }

    fn rustc_tables(&mut self, f: &mut dyn FnMut(&mut Tables<'_>)) {
        f(self)
    }

    fn ty_kind(&mut self, ty: crate::stable_mir::ty::Ty) -> TyKind {
        let ty = self.types[ty.0];
        ty.stable(self)
    }
}

pub struct Tables<'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub def_ids: Vec<DefId>,
    pub types: Vec<Ty<'tcx>>,
}

impl<'tcx> Tables<'tcx> {
    fn intern_ty(&mut self, ty: Ty<'tcx>) -> stable_mir::ty::Ty {
        if let Some(id) = self.types.iter().position(|&t| t == ty) {
            return stable_mir::ty::Ty(id);
        }
        let id = self.types.len();
        self.types.push(ty);
        stable_mir::ty::Ty(id)
    }
}

/// Build a stable mir crate from a given crate number.
fn smir_crate(tcx: TyCtxt<'_>, crate_num: CrateNum) -> stable_mir::Crate {
    let crate_name = tcx.crate_name(crate_num).to_string();
    let is_local = crate_num == LOCAL_CRATE;
    debug!(?crate_name, ?crate_num, "smir_crate");
    stable_mir::Crate { id: crate_num.into(), name: crate_name, is_local }
}

/// Trait used to convert between an internal MIR type to a Stable MIR type.
pub(crate) trait Stable<'tcx> {
    /// The stable representation of the type implementing Stable.
    type T;
    /// Converts an object to the equivalent Stable MIR representation.
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T;
}

impl<'tcx> Stable<'tcx> for mir::Statement<'tcx> {
    type T = stable_mir::mir::Statement;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use rustc_middle::mir::StatementKind::*;
        match &self.kind {
            Assign(assign) => {
                stable_mir::mir::Statement::Assign(assign.0.stable(tables), assign.1.stable(tables))
            }
            FakeRead(_) => todo!(),
            SetDiscriminant { .. } => todo!(),
            Deinit(_) => todo!(),
            StorageLive(_) => todo!(),
            StorageDead(_) => todo!(),
            Retag(_, _) => todo!(),
            PlaceMention(_) => todo!(),
            AscribeUserType(_, _) => todo!(),
            Coverage(_) => todo!(),
            Intrinsic(_) => todo!(),
            ConstEvalCounter => todo!(),
            Nop => stable_mir::mir::Statement::Nop,
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::Rvalue<'tcx> {
    type T = stable_mir::mir::Rvalue;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use mir::Rvalue::*;
        match self {
            Use(op) => stable_mir::mir::Rvalue::Use(op.stable(tables)),
            Repeat(_, _) => todo!(),
            Ref(region, kind, place) => stable_mir::mir::Rvalue::Ref(
                opaque(region),
                kind.stable(tables),
                place.stable(tables),
            ),
            ThreadLocalRef(def_id) => {
                stable_mir::mir::Rvalue::ThreadLocalRef(rustc_internal::crate_item(*def_id))
            }
            AddressOf(mutability, place) => {
                stable_mir::mir::Rvalue::AddressOf(mutability.stable(tables), place.stable(tables))
            }
            Len(place) => stable_mir::mir::Rvalue::Len(place.stable(tables)),
            Cast(_, _, _) => todo!(),
            BinaryOp(bin_op, ops) => stable_mir::mir::Rvalue::BinaryOp(
                bin_op.stable(tables),
                ops.0.stable(tables),
                ops.1.stable(tables),
            ),
            CheckedBinaryOp(bin_op, ops) => stable_mir::mir::Rvalue::CheckedBinaryOp(
                bin_op.stable(tables),
                ops.0.stable(tables),
                ops.1.stable(tables),
            ),
            NullaryOp(_, _) => todo!(),
            UnaryOp(un_op, op) => {
                stable_mir::mir::Rvalue::UnaryOp(un_op.stable(tables), op.stable(tables))
            }
            Discriminant(place) => stable_mir::mir::Rvalue::Discriminant(place.stable(tables)),
            Aggregate(_, _) => todo!(),
            ShallowInitBox(_, _) => todo!(),
            CopyForDeref(place) => stable_mir::mir::Rvalue::CopyForDeref(place.stable(tables)),
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::Mutability {
    type T = stable_mir::mir::Mutability;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        use mir::Mutability::*;
        match *self {
            Not => stable_mir::mir::Mutability::Not,
            Mut => stable_mir::mir::Mutability::Mut,
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::BorrowKind {
    type T = stable_mir::mir::BorrowKind;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use mir::BorrowKind::*;
        match *self {
            Shared => stable_mir::mir::BorrowKind::Shared,
            Shallow => stable_mir::mir::BorrowKind::Shallow,
            Mut { kind } => stable_mir::mir::BorrowKind::Mut { kind: kind.stable(tables) },
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::MutBorrowKind {
    type T = stable_mir::mir::MutBorrowKind;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        use mir::MutBorrowKind::*;
        match *self {
            Default => stable_mir::mir::MutBorrowKind::Default,
            TwoPhaseBorrow => stable_mir::mir::MutBorrowKind::TwoPhaseBorrow,
            ClosureCapture => stable_mir::mir::MutBorrowKind::ClosureCapture,
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::NullOp<'tcx> {
    type T = stable_mir::mir::NullOp;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use mir::NullOp::*;
        match self {
            SizeOf => stable_mir::mir::NullOp::SizeOf,
            AlignOf => stable_mir::mir::NullOp::AlignOf,
            OffsetOf(indices) => stable_mir::mir::NullOp::OffsetOf(
                indices.iter().map(|idx| idx.stable(tables)).collect(),
            ),
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::CastKind {
    type T = stable_mir::mir::CastKind;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use mir::CastKind::*;
        match self {
            PointerExposeAddress => stable_mir::mir::CastKind::PointerExposeAddress,
            PointerFromExposedAddress => stable_mir::mir::CastKind::PointerFromExposedAddress,
            PointerCoercion(c) => stable_mir::mir::CastKind::PointerCoercion(c.stable(tables)),
            DynStar => stable_mir::mir::CastKind::DynStar,
            IntToInt => stable_mir::mir::CastKind::IntToInt,
            FloatToInt => stable_mir::mir::CastKind::FloatToInt,
            FloatToFloat => stable_mir::mir::CastKind::FloatToFloat,
            IntToFloat => stable_mir::mir::CastKind::IntToFloat,
            PtrToPtr => stable_mir::mir::CastKind::PtrToPtr,
            FnPtrToPtr => stable_mir::mir::CastKind::FnPtrToPtr,
            Transmute => stable_mir::mir::CastKind::Transmute,
        }
    }
}

impl<'tcx> Stable<'tcx> for ty::adjustment::PointerCoercion {
    type T = stable_mir::mir::PointerCoercion;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use ty::adjustment::PointerCoercion;
        match self {
            PointerCoercion::ReifyFnPointer => stable_mir::mir::PointerCoercion::ReifyFnPointer,
            PointerCoercion::UnsafeFnPointer => stable_mir::mir::PointerCoercion::UnsafeFnPointer,
            PointerCoercion::ClosureFnPointer(unsafety) => {
                stable_mir::mir::PointerCoercion::ClosureFnPointer(unsafety.stable(tables))
            }
            PointerCoercion::MutToConstPointer => {
                stable_mir::mir::PointerCoercion::MutToConstPointer
            }
            PointerCoercion::ArrayToPointer => stable_mir::mir::PointerCoercion::ArrayToPointer,
            PointerCoercion::Unsize => stable_mir::mir::PointerCoercion::Unsize,
        }
    }
}

impl<'tcx> Stable<'tcx> for rustc_hir::Unsafety {
    type T = stable_mir::mir::Safety;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        match self {
            rustc_hir::Unsafety::Unsafe => stable_mir::mir::Safety::Unsafe,
            rustc_hir::Unsafety::Normal => stable_mir::mir::Safety::Normal,
        }
    }
}

impl<'tcx> Stable<'tcx> for FieldIdx {
    type T = usize;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        self.as_usize()
    }
}

impl<'tcx> Stable<'tcx> for mir::Operand<'tcx> {
    type T = stable_mir::mir::Operand;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use mir::Operand::*;
        match self {
            Copy(place) => stable_mir::mir::Operand::Copy(place.stable(tables)),
            Move(place) => stable_mir::mir::Operand::Move(place.stable(tables)),
            Constant(c) => stable_mir::mir::Operand::Constant(c.to_string()),
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::Place<'tcx> {
    type T = stable_mir::mir::Place;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        stable_mir::mir::Place {
            local: self.local.as_usize(),
            projection: format!("{:?}", self.projection),
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::UnwindAction {
    type T = stable_mir::mir::UnwindAction;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        use rustc_middle::mir::UnwindAction;
        match self {
            UnwindAction::Continue => stable_mir::mir::UnwindAction::Continue,
            UnwindAction::Unreachable => stable_mir::mir::UnwindAction::Unreachable,
            UnwindAction::Terminate => stable_mir::mir::UnwindAction::Terminate,
            UnwindAction::Cleanup(bb) => stable_mir::mir::UnwindAction::Cleanup(bb.as_usize()),
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::AssertMessage<'tcx> {
    type T = stable_mir::mir::AssertMessage;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use rustc_middle::mir::AssertKind;
        match self {
            AssertKind::BoundsCheck { len, index } => stable_mir::mir::AssertMessage::BoundsCheck {
                len: len.stable(tables),
                index: index.stable(tables),
            },
            AssertKind::Overflow(bin_op, op1, op2) => stable_mir::mir::AssertMessage::Overflow(
                bin_op.stable(tables),
                op1.stable(tables),
                op2.stable(tables),
            ),
            AssertKind::OverflowNeg(op) => {
                stable_mir::mir::AssertMessage::OverflowNeg(op.stable(tables))
            }
            AssertKind::DivisionByZero(op) => {
                stable_mir::mir::AssertMessage::DivisionByZero(op.stable(tables))
            }
            AssertKind::RemainderByZero(op) => {
                stable_mir::mir::AssertMessage::RemainderByZero(op.stable(tables))
            }
            AssertKind::ResumedAfterReturn(generator) => {
                stable_mir::mir::AssertMessage::ResumedAfterReturn(generator.stable(tables))
            }
            AssertKind::ResumedAfterPanic(generator) => {
                stable_mir::mir::AssertMessage::ResumedAfterPanic(generator.stable(tables))
            }
            AssertKind::MisalignedPointerDereference { required, found } => {
                stable_mir::mir::AssertMessage::MisalignedPointerDereference {
                    required: required.stable(tables),
                    found: found.stable(tables),
                }
            }
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::BinOp {
    type T = stable_mir::mir::BinOp;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        use mir::BinOp;
        match self {
            BinOp::Add => stable_mir::mir::BinOp::Add,
            BinOp::AddUnchecked => stable_mir::mir::BinOp::AddUnchecked,
            BinOp::Sub => stable_mir::mir::BinOp::Sub,
            BinOp::SubUnchecked => stable_mir::mir::BinOp::SubUnchecked,
            BinOp::Mul => stable_mir::mir::BinOp::Mul,
            BinOp::MulUnchecked => stable_mir::mir::BinOp::MulUnchecked,
            BinOp::Div => stable_mir::mir::BinOp::Div,
            BinOp::Rem => stable_mir::mir::BinOp::Rem,
            BinOp::BitXor => stable_mir::mir::BinOp::BitXor,
            BinOp::BitAnd => stable_mir::mir::BinOp::BitAnd,
            BinOp::BitOr => stable_mir::mir::BinOp::BitOr,
            BinOp::Shl => stable_mir::mir::BinOp::Shl,
            BinOp::ShlUnchecked => stable_mir::mir::BinOp::ShlUnchecked,
            BinOp::Shr => stable_mir::mir::BinOp::Shr,
            BinOp::ShrUnchecked => stable_mir::mir::BinOp::ShrUnchecked,
            BinOp::Eq => stable_mir::mir::BinOp::Eq,
            BinOp::Lt => stable_mir::mir::BinOp::Lt,
            BinOp::Le => stable_mir::mir::BinOp::Le,
            BinOp::Ne => stable_mir::mir::BinOp::Ne,
            BinOp::Ge => stable_mir::mir::BinOp::Ge,
            BinOp::Gt => stable_mir::mir::BinOp::Gt,
            BinOp::Offset => stable_mir::mir::BinOp::Offset,
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::UnOp {
    type T = stable_mir::mir::UnOp;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        use mir::UnOp;
        match self {
            UnOp::Not => stable_mir::mir::UnOp::Not,
            UnOp::Neg => stable_mir::mir::UnOp::Neg,
        }
    }
}

impl<'tcx> Stable<'tcx> for rustc_hir::GeneratorKind {
    type T = stable_mir::mir::GeneratorKind;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        use rustc_hir::{AsyncGeneratorKind, GeneratorKind};
        match self {
            GeneratorKind::Async(async_gen) => {
                let async_gen = match async_gen {
                    AsyncGeneratorKind::Block => stable_mir::mir::AsyncGeneratorKind::Block,
                    AsyncGeneratorKind::Closure => stable_mir::mir::AsyncGeneratorKind::Closure,
                    AsyncGeneratorKind::Fn => stable_mir::mir::AsyncGeneratorKind::Fn,
                };
                stable_mir::mir::GeneratorKind::Async(async_gen)
            }
            GeneratorKind::Gen => stable_mir::mir::GeneratorKind::Gen,
        }
    }
}

impl<'tcx> Stable<'tcx> for mir::InlineAsmOperand<'tcx> {
    type T = stable_mir::mir::InlineAsmOperand;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use rustc_middle::mir::InlineAsmOperand;

        let (in_value, out_place) = match self {
            InlineAsmOperand::In { value, .. } => (Some(value.stable(tables)), None),
            InlineAsmOperand::Out { place, .. } => (None, place.map(|place| place.stable(tables))),
            InlineAsmOperand::InOut { in_value, out_place, .. } => {
                (Some(in_value.stable(tables)), out_place.map(|place| place.stable(tables)))
            }
            InlineAsmOperand::Const { .. }
            | InlineAsmOperand::SymFn { .. }
            | InlineAsmOperand::SymStatic { .. } => (None, None),
        };

        stable_mir::mir::InlineAsmOperand { in_value, out_place, raw_rpr: format!("{:?}", self) }
    }
}

impl<'tcx> Stable<'tcx> for mir::Terminator<'tcx> {
    type T = stable_mir::mir::Terminator;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use rustc_middle::mir::TerminatorKind::*;
        use stable_mir::mir::Terminator;
        match &self.kind {
            Goto { target } => Terminator::Goto { target: target.as_usize() },
            SwitchInt { discr, targets } => Terminator::SwitchInt {
                discr: discr.stable(tables),
                targets: targets
                    .iter()
                    .map(|(value, target)| stable_mir::mir::SwitchTarget {
                        value,
                        target: target.as_usize(),
                    })
                    .collect(),
                otherwise: targets.otherwise().as_usize(),
            },
            Resume => Terminator::Resume,
            Terminate => Terminator::Abort,
            Return => Terminator::Return,
            Unreachable => Terminator::Unreachable,
            Drop { place, target, unwind, replace: _ } => Terminator::Drop {
                place: place.stable(tables),
                target: target.as_usize(),
                unwind: unwind.stable(tables),
            },
            Call { func, args, destination, target, unwind, call_source: _, fn_span: _ } => {
                Terminator::Call {
                    func: func.stable(tables),
                    args: args.iter().map(|arg| arg.stable(tables)).collect(),
                    destination: destination.stable(tables),
                    target: target.map(|t| t.as_usize()),
                    unwind: unwind.stable(tables),
                }
            }
            Assert { cond, expected, msg, target, unwind } => Terminator::Assert {
                cond: cond.stable(tables),
                expected: *expected,
                msg: msg.stable(tables),
                target: target.as_usize(),
                unwind: unwind.stable(tables),
            },
            InlineAsm { template, operands, options, line_spans, destination, unwind } => {
                Terminator::InlineAsm {
                    template: format!("{:?}", template),
                    operands: operands.iter().map(|operand| operand.stable(tables)).collect(),
                    options: format!("{:?}", options),
                    line_spans: format!("{:?}", line_spans),
                    destination: destination.map(|d| d.as_usize()),
                    unwind: unwind.stable(tables),
                }
            }
            Yield { .. } | GeneratorDrop | FalseEdge { .. } | FalseUnwind { .. } => unreachable!(),
        }
    }
}

impl<'tcx> Stable<'tcx> for ty::GenericArgs<'tcx> {
    type T = stable_mir::ty::GenericArgs;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use stable_mir::ty::{GenericArgKind, GenericArgs};

        GenericArgs(
            self.iter()
                .map(|arg| match arg.unpack() {
                    ty::GenericArgKind::Lifetime(region) => {
                        GenericArgKind::Lifetime(opaque(&region))
                    }
                    ty::GenericArgKind::Type(ty) => GenericArgKind::Type(tables.intern_ty(ty)),
                    ty::GenericArgKind::Const(const_) => GenericArgKind::Const(opaque(&const_)),
                })
                .collect(),
        )
    }
}

impl<'tcx> Stable<'tcx> for ty::PolyFnSig<'tcx> {
    type T = stable_mir::ty::PolyFnSig;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use stable_mir::ty::Binder;

        Binder {
            value: self.skip_binder().stable(tables),
            bound_vars: self
                .bound_vars()
                .iter()
                .map(|bound_var| bound_var.stable(tables))
                .collect(),
        }
    }
}

impl<'tcx> Stable<'tcx> for ty::FnSig<'tcx> {
    type T = stable_mir::ty::FnSig;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        use rustc_target::spec::abi;
        use stable_mir::ty::{Abi, FnSig, Unsafety};

        FnSig {
            inputs_and_output: self
                .inputs_and_output
                .iter()
                .map(|ty| tables.intern_ty(ty))
                .collect(),
            c_variadic: self.c_variadic,
            unsafety: match self.unsafety {
                hir::Unsafety::Normal => Unsafety::Normal,
                hir::Unsafety::Unsafe => Unsafety::Unsafe,
            },
            abi: match self.abi {
                abi::Abi::Rust => Abi::Rust,
                abi::Abi::C { unwind } => Abi::C { unwind },
                abi::Abi::Cdecl { unwind } => Abi::Cdecl { unwind },
                abi::Abi::Stdcall { unwind } => Abi::Stdcall { unwind },
                abi::Abi::Fastcall { unwind } => Abi::Fastcall { unwind },
                abi::Abi::Vectorcall { unwind } => Abi::Vectorcall { unwind },
                abi::Abi::Thiscall { unwind } => Abi::Thiscall { unwind },
                abi::Abi::Aapcs { unwind } => Abi::Aapcs { unwind },
                abi::Abi::Win64 { unwind } => Abi::Win64 { unwind },
                abi::Abi::SysV64 { unwind } => Abi::SysV64 { unwind },
                abi::Abi::PtxKernel => Abi::PtxKernel,
                abi::Abi::Msp430Interrupt => Abi::Msp430Interrupt,
                abi::Abi::X86Interrupt => Abi::X86Interrupt,
                abi::Abi::AmdGpuKernel => Abi::AmdGpuKernel,
                abi::Abi::EfiApi => Abi::EfiApi,
                abi::Abi::AvrInterrupt => Abi::AvrInterrupt,
                abi::Abi::AvrNonBlockingInterrupt => Abi::AvrNonBlockingInterrupt,
                abi::Abi::CCmseNonSecureCall => Abi::CCmseNonSecureCall,
                abi::Abi::Wasm => Abi::Wasm,
                abi::Abi::System { unwind } => Abi::System { unwind },
                abi::Abi::RustIntrinsic => Abi::RustIntrinsic,
                abi::Abi::RustCall => Abi::RustCall,
                abi::Abi::PlatformIntrinsic => Abi::PlatformIntrinsic,
                abi::Abi::Unadjusted => Abi::Unadjusted,
                abi::Abi::RustCold => Abi::RustCold,
            },
        }
    }
}

impl<'tcx> Stable<'tcx> for ty::BoundVariableKind {
    type T = stable_mir::ty::BoundVariableKind;
    fn stable(&self, _: &mut Tables<'tcx>) -> Self::T {
        use stable_mir::ty::{BoundRegionKind, BoundTyKind, BoundVariableKind};

        match self {
            ty::BoundVariableKind::Ty(bound_ty_kind) => {
                BoundVariableKind::Ty(match bound_ty_kind {
                    ty::BoundTyKind::Anon => BoundTyKind::Anon,
                    ty::BoundTyKind::Param(def_id, symbol) => {
                        BoundTyKind::Param(rustc_internal::param_def(*def_id), symbol.to_string())
                    }
                })
            }
            ty::BoundVariableKind::Region(bound_region_kind) => {
                BoundVariableKind::Region(match bound_region_kind {
                    ty::BoundRegionKind::BrAnon(option_span) => {
                        BoundRegionKind::BrAnon(option_span.map(|span| opaque(&span)))
                    }
                    ty::BoundRegionKind::BrNamed(def_id, symbol) => BoundRegionKind::BrNamed(
                        rustc_internal::br_named_def(*def_id),
                        symbol.to_string(),
                    ),
                    ty::BoundRegionKind::BrEnv => BoundRegionKind::BrEnv,
                })
            }
            ty::BoundVariableKind::Const => BoundVariableKind::Const,
        }
    }
}

impl<'tcx> Stable<'tcx> for Ty<'tcx> {
    type T = stable_mir::ty::TyKind;
    fn stable(&self, tables: &mut Tables<'tcx>) -> Self::T {
        match self.kind() {
            ty::Bool => TyKind::RigidTy(RigidTy::Bool),
            ty::Char => TyKind::RigidTy(RigidTy::Char),
            ty::Int(int_ty) => match int_ty {
                ty::IntTy::Isize => TyKind::RigidTy(RigidTy::Int(IntTy::Isize)),
                ty::IntTy::I8 => TyKind::RigidTy(RigidTy::Int(IntTy::I8)),
                ty::IntTy::I16 => TyKind::RigidTy(RigidTy::Int(IntTy::I16)),
                ty::IntTy::I32 => TyKind::RigidTy(RigidTy::Int(IntTy::I32)),
                ty::IntTy::I64 => TyKind::RigidTy(RigidTy::Int(IntTy::I64)),
                ty::IntTy::I128 => TyKind::RigidTy(RigidTy::Int(IntTy::I128)),
            },
            ty::Uint(uint_ty) => match uint_ty {
                ty::UintTy::Usize => TyKind::RigidTy(RigidTy::Uint(UintTy::Usize)),
                ty::UintTy::U8 => TyKind::RigidTy(RigidTy::Uint(UintTy::U8)),
                ty::UintTy::U16 => TyKind::RigidTy(RigidTy::Uint(UintTy::U16)),
                ty::UintTy::U32 => TyKind::RigidTy(RigidTy::Uint(UintTy::U32)),
                ty::UintTy::U64 => TyKind::RigidTy(RigidTy::Uint(UintTy::U64)),
                ty::UintTy::U128 => TyKind::RigidTy(RigidTy::Uint(UintTy::U128)),
            },
            ty::Float(float_ty) => match float_ty {
                ty::FloatTy::F32 => TyKind::RigidTy(RigidTy::Float(FloatTy::F32)),
                ty::FloatTy::F64 => TyKind::RigidTy(RigidTy::Float(FloatTy::F64)),
            },
            ty::Adt(adt_def, generic_args) => TyKind::RigidTy(RigidTy::Adt(
                rustc_internal::adt_def(adt_def.did()),
                generic_args.stable(tables),
            )),
            ty::Foreign(def_id) => {
                TyKind::RigidTy(RigidTy::Foreign(rustc_internal::foreign_def(*def_id)))
            }
            ty::Str => TyKind::RigidTy(RigidTy::Str),
            ty::Array(ty, constant) => {
                TyKind::RigidTy(RigidTy::Array(tables.intern_ty(*ty), opaque(constant)))
            }
            ty::Slice(ty) => TyKind::RigidTy(RigidTy::Slice(tables.intern_ty(*ty))),
            ty::RawPtr(ty::TypeAndMut { ty, mutbl }) => {
                TyKind::RigidTy(RigidTy::RawPtr(tables.intern_ty(*ty), mutbl.stable(tables)))
            }
            ty::Ref(region, ty, mutbl) => TyKind::RigidTy(RigidTy::Ref(
                opaque(region),
                tables.intern_ty(*ty),
                mutbl.stable(tables),
            )),
            ty::FnDef(def_id, generic_args) => TyKind::RigidTy(RigidTy::FnDef(
                rustc_internal::fn_def(*def_id),
                generic_args.stable(tables),
            )),
            ty::FnPtr(poly_fn_sig) => TyKind::RigidTy(RigidTy::FnPtr(poly_fn_sig.stable(tables))),
            ty::Dynamic(_, _, _) => todo!(),
            ty::Closure(def_id, generic_args) => TyKind::RigidTy(RigidTy::Closure(
                rustc_internal::closure_def(*def_id),
                generic_args.stable(tables),
            )),
            ty::Generator(def_id, generic_args, movability) => TyKind::RigidTy(RigidTy::Generator(
                rustc_internal::generator_def(*def_id),
                generic_args.stable(tables),
                match movability {
                    hir::Movability::Static => Movability::Static,
                    hir::Movability::Movable => Movability::Movable,
                },
            )),
            ty::Never => TyKind::RigidTy(RigidTy::Never),
            ty::Tuple(fields) => TyKind::RigidTy(RigidTy::Tuple(
                fields.iter().map(|ty| tables.intern_ty(ty)).collect(),
            )),
            ty::Alias(_, _) => todo!(),
            ty::Param(_) => todo!(),
            ty::Bound(_, _) => todo!(),
            ty::Placeholder(..)
            | ty::GeneratorWitness(_)
            | ty::GeneratorWitnessMIR(_, _)
            | ty::Infer(_)
            | ty::Error(_) => {
                unreachable!();
            }
        }
    }
}
