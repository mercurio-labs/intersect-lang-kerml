//! Handwritten evaluation contexts; the last frame is the innermost.
use crate::{EvalError, EvalLimits, ObjId, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Clone)]
pub(crate) enum Frame {
    Type(ObjId),
    Arguments {
        expression: ObjId,
        bound: Vec<(ObjId, Vec<Value>)>,
    },
    /// A frozen representation of Pilot's freshly instantiated invocation.
    /// The callee type also hides omitted parameters from outer recursive calls.
    Call {
        invocation: ObjId,
        ty: ObjId,
        bound: Vec<(ObjId, Vec<Value>)>,
    },
    /// The Pilot's metaclass feature, represented without creating an object.
    Metaclass {
        element: ObjId,
        self_feature: Option<ObjId>,
    },
}
impl Frame {
    pub fn identity(&self) -> ObjId {
        match self {
            Self::Type(t) => *t,
            Self::Arguments { expression, .. } => *expression,
            Self::Call { invocation, .. } => *invocation,
            Self::Metaclass { element, .. } => *element,
        }
    }
}
#[derive(Default)]
pub(crate) struct Run<'o> {
    pub steps: Rc<Cell<u64>>,
    pub depth: Cell<u32>,
    pub observer: RefCell<Option<&'o mut dyn crate::EvalObserver>>,
}
#[derive(Clone, Default)]
pub(crate) struct Context<'o> {
    pub frames: Vec<Frame>,
    pub run: Rc<Run<'o>>,
}
impl Context<'_> {
    pub fn observe<T>(
        &self,
        f: impl FnOnce(&mut dyn crate::EvalObserver) -> T,
    ) -> Result<Option<T>, EvalError> {
        let mut observer = self
            .run
            .observer
            .try_borrow_mut()
            .map_err(|_| EvalError::Model {
                object: "evaluation observer".into(),
                message: "reentrant observer callback".into(),
            })?;
        match observer.as_mut() {
            Some(observer) => Ok(Some(f(&mut **observer))),
            None => Ok(None),
        }
    }
    pub fn skipped(&self, inv: ObjId, operand: usize) -> Result<(), EvalError> {
        self.observe(|o| o.skipped(inv, operand))?;
        Ok(())
    }
    pub fn pushed(&self, frame: Frame) -> Self {
        let mut next = self.clone();
        next.frames.push(frame);
        next
    }
    pub fn require_steps(&self, count: u64, limits: EvalLimits) -> Result<(), EvalError> {
        if count > limits.steps.saturating_sub(self.run.steps.get()) {
            Err(EvalError::StepLimit(limits.steps))
        } else {
            Ok(())
        }
    }
    pub fn step(&self, limits: EvalLimits) -> Result<u64, EvalError> {
        let n = self.run.steps.get();
        if n >= limits.steps {
            return Err(EvalError::StepLimit(limits.steps));
        }
        let n = n.checked_add(1).ok_or(EvalError::StepLimit(limits.steps))?;
        self.run.steps.set(n);
        Ok(n)
    }
}
