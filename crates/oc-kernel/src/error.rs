//! Kernel errors. Every failure of a kernel call is one of these; none panics and none
//! returns a body that failed validation.

use std::fmt;

/// Which operation failed (for messages and statistics).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpKind {
    Primitive,
    Prism,
    Revolve,
    Sweep,
    Loft,
    Boolean,
    Fillet,
    Chamfer,
    Shell,
    Draft,
    Offset,
    Transform,
    Compound,
    Section,
    Heal,
    Query,
    Tessellate,
    StepRead,
    StepWrite,
}

impl fmt::Display for OpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum KernelError {
    /// The arguments are unusable before any geometry is attempted: an index out of range,
    /// an open profile, a zero-length vector, a non-positive radius.
    InvalidInput { op: OpKind, detail: String },
    /// The kernel algorithm could not produce a result (a fillet larger than its faces
    /// allow, a shell whose offset surfaces collapse).
    Failed { op: OpKind, detail: String },
    /// The kernel produced a result that is not a valid solid, and healing could not fix
    /// it without changing the part. The result is discarded.
    InvalidResult { op: OpKind, problems: Vec<String> },
    /// The result is empty (a cut removed everything, a common of disjoint bodies).
    Empty { op: OpKind },
    /// Data exchange failed (unreadable STEP, nothing transferable).
    Io { op: OpKind, detail: String },
}

impl KernelError {
    pub fn op(&self) -> OpKind {
        match self {
            KernelError::InvalidInput { op, .. }
            | KernelError::Failed { op, .. }
            | KernelError::InvalidResult { op, .. }
            | KernelError::Empty { op }
            | KernelError::Io { op, .. } => *op,
        }
    }

    pub fn input(op: OpKind, detail: impl Into<String>) -> Self {
        KernelError::InvalidInput { op, detail: detail.into() }
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::InvalidInput { op, detail } => write!(f, "{op}: invalid input: {detail}"),
            KernelError::Failed { op, detail } => write!(f, "{op}: failed: {detail}"),
            KernelError::InvalidResult { op, problems } => {
                write!(f, "{op}: produced an invalid solid ({})", problems.join(", "))
            }
            KernelError::Empty { op } => write!(f, "{op}: the result is empty"),
            KernelError::Io { op, detail } => write!(f, "{op}: {detail}"),
        }
    }
}

impl std::error::Error for KernelError {}

pub type KResult<T> = Result<T, KernelError>;
