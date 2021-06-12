use super::*;
use byteorder::WriteBytesExt;

/// A frame represents the state of the stack and local variables at any location in the bytecode
///
/// In order to load bytecode into the JVM, the JVM requires that methods be annotated with
/// `StackMapTable` attributes to describe the state of the frame at offsets that can be jumped to.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct Frame<Cls, U> {
    /// Local variables in the frame
    pub locals: OffsetVec<VerificationType<Cls, U>>,

    /// Stack in the frame
    pub stack: OffsetVec<VerificationType<Cls, U>>,
}

impl Frame<RefType, (RefType, Offset)> {
    /// Update the frame to reflect the effects of the given (non-branching) instruction
    ///
    ///   * `insn_offset_in_basic_block` - used in uninitialized verification types
    ///   * `class_graph` - used to check whether types are assignable
    ///   * `constants_reader` - used to query constants information from constant indices
    ///   * `this_class` - used to determine the type of `UninitializedThis` after `<init>`
    ///
    pub fn interpret_instruction(
        &mut self,
        insn: &Instruction,
        insn_offset_in_block: Offset,
        class_graph: &ClassGraph,
        constants_reader: &ConstantsPool,
        this_class: &RefType,
    ) -> Result<(), VerifierErrorKind> {
        interpret_instruction(
            self,
            constants_reader,
            class_graph,
            this_class,
            insn,
            insn_offset_in_block,
        )
    }

    /// Update the frame to reflect the effects of the given branching instruction
    ///
    ///   * `class_graph` - used to check whether types are assignable
    ///   * `this_method_return_type` - used to check typecheck return instructions
    ///
    pub fn interpret_branch_instruction<Lbl, LblWide, LblNext>(
        &mut self,
        insn: &BranchInstruction<Lbl, LblWide, LblNext>,
        class_graph: &ClassGraph,
        this_method_return_type: &Option<FieldType>,
    ) -> Result<(), VerifierErrorKind> {
        interpret_branch_instruction(self, class_graph, this_method_return_type, insn)
    }

    /// Update the maximum locals and stack
    pub fn update_maximums(&self, max_locals: &mut Offset, max_stack: &mut Offset) {
        max_locals.0 = max_locals.0.max(self.locals.offset_len().0);
        max_stack.0 = max_stack.0.max(self.stack.offset_len().0);
    }

    /// Resolve the frame into its serializable form
    pub fn into_serializable(
        &self,
        constants_pool: &mut ConstantsPool,
        block_offset: Offset,
    ) -> Result<Frame<ClassConstantIndex, u16>, Error> {
        Ok(Frame {
            stack: self
                .stack
                .iter()
                .map(|(_, _, t)| t.into_serializable(constants_pool, block_offset))
                .collect::<Result<_, _>>()?,
            locals: self
                .locals
                .iter()
                .map(|(_, _, t)| t.into_serializable(constants_pool, block_offset))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl Frame<ClassConstantIndex, u16> {
    /// Compute a stack map frame for this frame, given the previous frame
    ///
    /// This will only use the `Full` option if none of the other stack map frame variants are
    /// enough to encode the transition.
    pub fn stack_map_frame(&self, offset_delta: u16, previous_frame: &Self) -> StackMapFrame {
        match self.stack.len() {
            0 => {
                let this_locals_len = self.locals.len();
                let prev_locals_len = previous_frame.locals.len();

                if this_locals_len <= prev_locals_len {
                    let len_difference = prev_locals_len - this_locals_len;
                    if len_difference < 4 {
                        let this_is_prefix_of_pref = self
                            .locals
                            .iter()
                            .zip(previous_frame.locals.iter())
                            .all(|((_, _, t1), (_, _, t2))| t1 == t2);

                        if this_is_prefix_of_pref {
                            if len_difference == 0 {
                                return StackMapFrame::SameLocalsNoStack { offset_delta };
                            } else {
                                return StackMapFrame::ChopLocalsNoStack {
                                    offset_delta,
                                    chopped_k: len_difference as u8,
                                };
                            }
                        }
                    }
                } else {
                    if this_locals_len - prev_locals_len < 4 {
                        let mut this_iter = self.locals.iter().map(|(_, _, t)| t);
                        let mut prev_is_prefix_of_this = true;
                        for (_, _, t1) in previous_frame.locals.iter() {
                            let t2 = this_iter.next().unwrap();
                            if t1 != t2 {
                                prev_is_prefix_of_this = false;
                                break;
                            }
                        }

                        if prev_is_prefix_of_this {
                            return StackMapFrame::AppendLocalsNoStack {
                                offset_delta,
                                locals: this_iter.cloned().collect(),
                            };
                        }
                    }
                }
            }
            1 if self.locals == previous_frame.locals => {
                return StackMapFrame::SameLocalsOneStack {
                    offset_delta,
                    stack: self.stack.iter().map(|(_, _, t)| t.clone()).next().unwrap(),
                }
            }
            _ => (),
        }

        self.full_stack_map_frame(offset_delta)
    }

    /// Compute a full stack map frame
    pub fn full_stack_map_frame(&self, offset_delta: u16) -> StackMapFrame {
        StackMapFrame::Full {
            offset_delta,
            stack: self.stack.iter().map(|(_, _, t)| t.clone()).collect(),
            locals: self.locals.iter().map(|(_, _, t)| t.clone()).collect(),
        }
    }
}

/// These types are from [this hierarchy][0]
///
/// [0]: https://docs.oracle.com/javase/specs/jvms/se7/html/jvms-4.html#jvms-4.10.1.2
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum VerificationType<Cls, U> {
    Integer,
    Float,
    Double,
    Long,
    Null,

    /// In the constructor, the `this` parameter starts with this type then turns into an object
    /// type after `<init>` is called
    UninitializedThis,

    /// Object type
    Object(Cls),

    /// State of an object after `new` has been called by `<init>` has not been called
    ///
    ///   - while we are building up the CFG, we use `(RefType, Offset)` for `U`, tracking the
    ///     type of the uninitialized object (which we get from the `new` instruction) and the
    ///     offset of the `new` instruction in that basic block.
    ///   - when serializing into a classfile, we use `u16` for `U`, corresponding to the offset of
    ///     the `new` instruction from the start of the method body
    Uninitialized(U),
}

impl<Cls, U> VerificationType<Cls, U> {
    /// Is this type is a reference type?
    pub fn is_reference(&self) -> bool {
        match self {
            VerificationType::Integer
            | VerificationType::Float
            | VerificationType::Double
            | VerificationType::Long => false,

            VerificationType::Null
            | VerificationType::UninitializedThis
            | VerificationType::Object(_)
            | VerificationType::Uninitialized(_) => true,
        }
    }
}

impl<U> From<FieldType> for VerificationType<RefType, U> {
    fn from(field_type: FieldType) -> Self {
        match field_type {
            FieldType::Base(BaseType::Int)
            | FieldType::Base(BaseType::Char)
            | FieldType::Base(BaseType::Short)
            | FieldType::Base(BaseType::Byte)
            | FieldType::Base(BaseType::Boolean) => VerificationType::Integer,
            FieldType::Base(BaseType::Float) => VerificationType::Float,
            FieldType::Base(BaseType::Long) => VerificationType::Long,
            FieldType::Base(BaseType::Double) => VerificationType::Double,
            FieldType::Ref(ref_type) => VerificationType::Object(ref_type),
        }
    }
}

impl Serialize for VerificationType<ClassConstantIndex, u16> {
    fn serialize<W: WriteBytesExt>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            VerificationType::Integer => 1u8.serialize(writer)?,
            VerificationType::Float => 2u8.serialize(writer)?,
            VerificationType::Double => 3u8.serialize(writer)?,
            VerificationType::Long => 4u8.serialize(writer)?,
            VerificationType::Null => 5u8.serialize(writer)?,
            VerificationType::UninitializedThis => 6u8.serialize(writer)?,
            VerificationType::Object(cls) => {
                7u8.serialize(writer)?;
                cls.serialize(writer)?;
            }
            VerificationType::Uninitialized(off) => {
                8u8.serialize(writer)?;
                off.serialize(writer)?;
            }
        };
        Ok(())
    }
}

impl<Cls, A> Width for VerificationType<Cls, A> {
    fn width(&self) -> usize {
        match self {
            VerificationType::Double | VerificationType::Long => 2,
            _ => 1,
        }
    }
}

impl<U> VerificationType<RefType, U> {
    /// Check if one verification type is assignable to another
    ///
    /// TODO: there is no handling of uninitialized yet. This just means that we might get false
    /// verification failures.
    pub fn is_assignable(class_graph: &ClassGraph, sub_type: &Self, super_type: &Self) -> bool {
        match (sub_type, super_type) {
            (Self::Integer, Self::Integer) => true,
            (Self::Float, Self::Float) => true,
            (Self::Long, Self::Long) => true,
            (Self::Double, Self::Double) => true,
            (Self::Null, Self::Null) => true,
            (Self::Null, Self::Object(_)) => true,
            (Self::Object(t1), Self::Object(t2)) => class_graph.is_java_assignable(t1, t2),
            _ => false,
        }
    }
}

impl VerificationType<RefType, (RefType, Offset)> {
    /// Resolve the type into its serializable form
    fn into_serializable(
        &self,
        constants_pool: &mut ConstantsPool,
        block_offset: Offset,
    ) -> Result<VerificationType<ClassConstantIndex, u16>, Error> {
        match self {
            VerificationType::Integer => Ok(VerificationType::Integer),
            VerificationType::Float => Ok(VerificationType::Float),
            VerificationType::Long => Ok(VerificationType::Long),
            VerificationType::Double => Ok(VerificationType::Double),
            VerificationType::Null => Ok(VerificationType::Null),
            VerificationType::UninitializedThis => Ok(VerificationType::UninitializedThis),
            VerificationType::Object(ref_type) => {
                let utf8_index = constants_pool.get_utf8(ref_type.render_class_info())?;
                let class_index = constants_pool.get_class(utf8_index)?;
                Ok(VerificationType::Object(class_index))
            }
            VerificationType::Uninitialized((_, Offset(offset_in_block))) => Ok(
                VerificationType::Uninitialized((block_offset.0 + offset_in_block) as u16),
            ),
        }
    }
}

pub trait ConstantsReader {
    /// Lookup the type of a value loaded from the constant pool (eg. from `ldc`)
    fn lookup_constant_type(&self, index: ConstantIndex) -> Result<FieldType, VerifierErrorKind>;

    /// Resolve the class
    fn lookup_class_reftype(&self, index: ClassConstantIndex)
        -> Result<RefType, VerifierErrorKind>;

    /// Resolve the class and the field type
    fn lookup_field(
        &self,
        index: FieldRefConstantIndex,
    ) -> Result<(RefType, FieldType), VerifierErrorKind>;

    /// Resolve a method into:
    ///
    ///   * it's receiver
    ///   * whether it is an interface
    ///   * whether it is an `<init>` method
    ///   * and its descriptor
    ///
    fn lookup_method(
        &self,
        index: MethodRefConstantIndex,
    ) -> Result<(ClassConstantIndex, bool, bool, MethodDescriptor), VerifierErrorKind>;

    /// Resolve the type of an invoke dynamic constant
    fn lookup_invoke_dynamic(
        &self,
        index: InvokeDynamicConstantIndex,
    ) -> Result<MethodDescriptor, VerifierErrorKind>;
}

fn interpret_instruction(
    frame: &mut Frame<RefType, (RefType, Offset)>,
    constants_reader: &impl ConstantsReader,
    class_graph: &ClassGraph,
    this_class: &RefType,
    insn: &Instruction,
    insn_offset_in_basic_block: Offset,
) -> Result<(), VerifierErrorKind> {
    use Instruction::*;
    use VerificationType::*;

    type VType = VerificationType<RefType, (RefType, Offset)>;
    let Frame {
        ref mut stack,
        ref mut locals,
    } = frame;

    match insn {
        Nop => (),
        AConstNull => {
            stack.push(Null);
        }
        IConstM1 | IConst0 | IConst1 | IConst2 | IConst3 | IConst4 | IConst5 => {
            stack.push(Integer);
        }
        LConst0 | LConst1 => {
            stack.push(Long);
        }
        FConst0 | FConst1 | FConst2 => {
            stack.push(Float);
        }
        DConst0 | DConst1 => {
            stack.push(Double);
        }
        BiPush(_) | SiPush(_) => {
            stack.push(Integer);
        }
        Ldc(idx) => {
            let typ: VType = constants_reader.lookup_constant_type(*idx)?.into();
            match typ.width() {
                1 => stack.push(typ),
                other => return Err(VerifierErrorKind::InvalidWidth(other)),
            };
        }
        Ldc2(idx) => {
            let typ: VType = constants_reader.lookup_constant_type(*idx)?.into();
            match typ.width() {
                2 => stack.push(typ),
                other => return Err(VerifierErrorKind::InvalidWidth(other)),
            };
        }

        ILoad(offset) => {
            get_local_expecting_type(locals, *offset, Integer)?;
            stack.push(Integer);
        }
        LLoad(offset) => {
            get_local_expecting_type(locals, *offset, Long)?;
            stack.push(Long);
        }
        FLoad(offset) => {
            get_local_expecting_type(locals, *offset, Float)?;
            stack.push(Float);
        }
        DLoad(offset) => {
            get_local_expecting_type(locals, *offset, Double)?;
            stack.push(Double);
        }
        ALoad(offset) => {
            let typ = get_local(locals, *offset)?.clone();
            stack.push(typ);
        }

        IALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::INT)))?;
            stack.push(Integer);
        }
        LALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::LONG)))?;
            stack.push(Long);
        }
        FALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::FLOAT)))?;
            stack.push(Float);
        }
        DALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::DOUBLE)))?;
            stack.push(Double);
        }
        AALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            let array_type = pop_offset_vec(stack)?;
            match array_type {
                Object(RefType::Array(elem_ty)) => match *elem_ty {
                    FieldType::Ref(ref_type) => stack.push(Object(ref_type)),
                    _ => return Err(VerifierErrorKind::InvalidType),
                },
                _ => return Err(VerifierErrorKind::InvalidType),
            };
        }
        BALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::BYTE)))?;
            stack.push(Integer);
        }
        CALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::CHAR)))?;
            stack.push(Integer);
        }
        SALoad => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::SHORT)))?;
            stack.push(Integer);
        }

        IStore(offset) => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            update_local_type(locals, *offset, Integer)?;
        }
        FStore(offset) => {
            pop_offset_vec_expecting_type(stack, Float)?;
            update_local_type(locals, *offset, Float)?;
        }
        LStore(offset) => {
            pop_offset_vec_expecting_type(stack, Long)?;
            update_local_type(locals, *offset, Long)?;
        }
        DStore(offset) => {
            pop_offset_vec_expecting_type(stack, Double)?;
            update_local_type(locals, *offset, Double)?;
        }
        AStore(offset) => {
            let popped_type = pop_offset_vec(stack)?;
            update_local_type(locals, *offset, popped_type)?;
        }

        IKill(offset) => {
            match locals.iter().last() {
                Some((Offset(last), _, Integer)) if last == *offset as usize => locals.pop(),
                _ => return Err(VerifierErrorKind::InvalidIndex),
            };
        }
        FKill(offset) => {
            match locals.iter().last() {
                Some((Offset(last), _, Float)) if last == *offset as usize => locals.pop(),
                _ => return Err(VerifierErrorKind::InvalidIndex),
            };
        }
        LKill(offset) => {
            match locals.iter().last() {
                Some((Offset(last), _, Long)) if last == *offset as usize => locals.pop(),
                _ => return Err(VerifierErrorKind::InvalidIndex),
            };
        }
        DKill(offset) => {
            match locals.iter().last() {
                Some((Offset(last), _, Double)) if last == *offset as usize => locals.pop(),
                _ => return Err(VerifierErrorKind::InvalidIndex),
            };
        }
        AKill(offset) => {
            match locals.iter().last() {
                Some((Offset(last), _, local_ty))
                    if last == *offset as usize && local_ty.is_reference() =>
                {
                    locals.pop()
                }
                _ => return Err(VerifierErrorKind::InvalidIndex),
            };
        }

        AHint(general_type) => {
            let general_type = VType::Object(general_type.clone());
            let specific_type = pop_offset_vec(stack)?;
            let is_valid_weakening =
                VerificationType::is_assignable(class_graph, &specific_type, &general_type);
            if is_valid_weakening {
                stack.push(general_type);
            } else {
                return Err(VerifierErrorKind::InvalidType);
            }
        }

        IAStore => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::INT)))?;
        }
        LAStore => {
            pop_offset_vec_expecting_type(stack, Long)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::LONG)))?;
        }
        FAStore => {
            pop_offset_vec_expecting_type(stack, Float)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::FLOAT)))?;
        }
        DAStore => {
            pop_offset_vec_expecting_type(stack, Double)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::DOUBLE)))?;
        }
        AAStore => {
            let elem_type = pop_offset_vec(stack)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            let array_type = pop_offset_vec(stack)?;
            match array_type {
                Object(RefType::Array(expected_elem_type))
                    if VerificationType::is_assignable(
                        class_graph,
                        &elem_type,
                        &VType::from(*expected_elem_type.clone()),
                    ) =>
                {
                    ()
                }
                _ => return Err(VerifierErrorKind::InvalidType),
            };
        }
        BAStore => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::BYTE)))?;
        }
        CAStore => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::CHAR)))?;
        }
        SAStore => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Object(RefType::array(FieldType::SHORT)))?;
        }

        Pop => {
            let _ = pop_offset_vec_expecting_width(stack, 1)?;
        }

        Pop2 => {
            let arg1 = pop_offset_vec(stack)?;
            match arg1.width() {
                // Form 1
                1 => {
                    let _ = pop_offset_vec_expecting_width(stack, 1)?;
                }

                // Form 2
                2 => (),

                other => return Err(VerifierErrorKind::InvalidWidth(other)),
            }
        }

        Dup => {
            let arg1 = pop_offset_vec_expecting_width(stack, 1)?;
            stack.push(arg1.clone());
            stack.push(arg1);
        }

        DupX1 => {
            let arg1 = pop_offset_vec_expecting_width(stack, 1)?;
            let arg2 = pop_offset_vec_expecting_width(stack, 1)?;
            stack.push(arg1.clone());
            stack.push(arg2);
            stack.push(arg1);
        }

        DupX2 => {
            let arg1 = pop_offset_vec_expecting_width(stack, 1)?;
            let arg2 = pop_offset_vec(stack)?;
            match arg2.width() {
                // Form 1
                1 => {
                    let arg3 = pop_offset_vec_expecting_width(stack, 1)?;
                    stack.push(arg1.clone());
                    stack.push(arg3);
                    stack.push(arg2);
                    stack.push(arg1);
                }

                // Form 2
                2 => {
                    stack.push(arg1.clone());
                    stack.push(arg2);
                    stack.push(arg1);
                }

                other => return Err(VerifierErrorKind::InvalidWidth(other)),
            }
        }

        Dup2 => {
            let arg1 = pop_offset_vec(stack)?;
            match arg1.width() {
                // Form 1
                1 => {
                    let arg2 = pop_offset_vec_expecting_width(stack, 1)?;
                    stack.push(arg2.clone());
                    stack.push(arg1.clone());
                    stack.push(arg2);
                    stack.push(arg1);
                }

                // Form 2
                2 => {
                    stack.push(arg1.clone());
                    stack.push(arg1);
                }

                other => return Err(VerifierErrorKind::InvalidWidth(other)),
            }
        }

        Dup2X1 => {
            let arg1 = pop_offset_vec(stack)?;
            let arg2 = pop_offset_vec_expecting_width(stack, 1)?;
            match arg1.width() {
                // Form 1
                1 => {
                    let arg3 = pop_offset_vec_expecting_width(stack, 1)?;
                    stack.push(arg2.clone());
                    stack.push(arg1.clone());
                    stack.push(arg3);
                    stack.push(arg2);
                    stack.push(arg1);
                }

                // Form 2
                2 => {
                    stack.push(arg1.clone());
                    stack.push(arg2);
                    stack.push(arg1);
                }

                other => return Err(VerifierErrorKind::InvalidWidth(other)),
            }
        }

        Dup2X2 => {
            let arg1 = pop_offset_vec(stack)?;
            match arg1.width() {
                1 => {
                    let arg2 = pop_offset_vec_expecting_width(stack, 1)?;
                    let arg3 = pop_offset_vec(stack)?;
                    match arg3.width() {
                        // Form 1
                        1 => {
                            let arg4 = pop_offset_vec_expecting_width(stack, 1)?;
                            stack.push(arg2.clone());
                            stack.push(arg1.clone());
                            stack.push(arg4);
                            stack.push(arg3);
                            stack.push(arg2);
                            stack.push(arg1);
                        }

                        // Form 3
                        2 => {
                            stack.push(arg2.clone());
                            stack.push(arg1.clone());
                            stack.push(arg3);
                            stack.push(arg2);
                            stack.push(arg1);
                        }

                        other => return Err(VerifierErrorKind::InvalidWidth(other)),
                    }
                }

                2 => {
                    let arg2 = pop_offset_vec(stack)?;
                    match arg2.width() {
                        // Form 2
                        1 => {
                            let arg3 = pop_offset_vec_expecting_width(stack, 1)?;
                            stack.push(arg1.clone());
                            stack.push(arg3);
                            stack.push(arg2);
                            stack.push(arg1);
                        }

                        // Form 4
                        2 => {
                            stack.push(arg1.clone());
                            stack.push(arg2);
                            stack.push(arg1);
                        }

                        other => return Err(VerifierErrorKind::InvalidWidth(other)),
                    }
                }

                other => return Err(VerifierErrorKind::InvalidWidth(other)),
            }
        }

        Swap => {
            let arg1 = pop_offset_vec_expecting_width(stack, 1)?;
            let arg2 = pop_offset_vec_expecting_width(stack, 1)?;
            stack.push(arg1);
            stack.push(arg2);
        }

        IAdd | ISub | IDiv | IMul | IRem | IAnd | IOr | IXor | ISh(_) => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
            stack.push(Integer);
        }

        LAdd | LSub | LDiv | LMul | LRem | LAnd | LOr | LXor => {
            pop_offset_vec_expecting_type(stack, Long)?;
            pop_offset_vec_expecting_type(stack, Long)?;
            stack.push(Long);
        }

        FAdd | FSub | FDiv | FMul | FRem => {
            pop_offset_vec_expecting_type(stack, Float)?;
            pop_offset_vec_expecting_type(stack, Float)?;
            stack.push(Float);
        }

        DAdd | DSub | DDiv | DMul | DRem => {
            pop_offset_vec_expecting_type(stack, Double)?;
            pop_offset_vec_expecting_type(stack, Double)?;
            stack.push(Double);
        }

        INeg | I2B | I2C | I2S => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            stack.push(Integer);
        }

        LNeg => {
            pop_offset_vec_expecting_type(stack, Long)?;
            stack.push(Long);
        }

        FNeg => {
            pop_offset_vec_expecting_type(stack, Float)?;
            stack.push(Float);
        }

        DNeg => {
            pop_offset_vec_expecting_type(stack, Double)?;
            stack.push(Double);
        }

        LSh(_) => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Long)?;
            stack.push(Long);
        }

        IInc(offset, _) => {
            get_local_expecting_type(locals, *offset, Integer)?;
        }

        I2L => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            stack.push(Long);
        }
        I2F => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            stack.push(Float);
        }
        I2D => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            stack.push(Double);
        }

        L2I => {
            pop_offset_vec_expecting_type(stack, Long)?;
            stack.push(Integer);
        }
        L2F => {
            pop_offset_vec_expecting_type(stack, Long)?;
            stack.push(Float);
        }
        L2D => {
            pop_offset_vec_expecting_type(stack, Long)?;
            stack.push(Double);
        }

        F2I => {
            pop_offset_vec_expecting_type(stack, Float)?;
            stack.push(Integer);
        }
        F2L => {
            pop_offset_vec_expecting_type(stack, Float)?;
            stack.push(Long);
        }
        F2D => {
            pop_offset_vec_expecting_type(stack, Float)?;
            stack.push(Double);
        }

        D2I => {
            pop_offset_vec_expecting_type(stack, Double)?;
            stack.push(Integer);
        }
        D2L => {
            pop_offset_vec_expecting_type(stack, Double)?;
            stack.push(Long);
        }
        D2F => {
            pop_offset_vec_expecting_type(stack, Double)?;
            stack.push(Float);
        }

        LCmp => {
            pop_offset_vec_expecting_type(stack, Long)?;
            pop_offset_vec_expecting_type(stack, Long)?;
            stack.push(Integer);
        }
        FCmp(_) => {
            pop_offset_vec_expecting_type(stack, Float)?;
            pop_offset_vec_expecting_type(stack, Float)?;
            stack.push(Integer);
        }
        DCmp(_) => {
            pop_offset_vec_expecting_type(stack, Double)?;
            pop_offset_vec_expecting_type(stack, Double)?;
            stack.push(Integer);
        }

        GetStatic(field_ref) => {
            let (_, field_type) = constants_reader.lookup_field(*field_ref)?;
            stack.push(field_type.into());
        }
        PutStatic(field_ref) => {
            let (_, field_type) = constants_reader.lookup_field(*field_ref)?;
            let arg_type = pop_offset_vec(stack)?;
            if !VerificationType::is_assignable(&class_graph, &arg_type, &VType::from(field_type)) {
                return Err(VerifierErrorKind::InvalidType);
            }
        }

        GetField(field_ref) => {
            let (object_type, field_type) = constants_reader.lookup_field(*field_ref)?;
            let object_type_found = pop_offset_vec(stack)?;
            if !VerificationType::is_assignable(
                &class_graph,
                &object_type_found,
                &VType::from(FieldType::Ref(object_type)),
            ) {
                return Err(VerifierErrorKind::InvalidType);
            }
            stack.push(field_type.into());
        }
        PutField(field_ref) => {
            let (object_type, field_type) = constants_reader.lookup_field(*field_ref)?;
            let arg_type = pop_offset_vec(stack)?;
            let object_type_found = pop_offset_vec(stack)?;
            if !VerificationType::is_assignable(&class_graph, &arg_type, &VType::from(field_type))
                || !VerificationType::is_assignable(
                    &class_graph,
                    &object_type_found,
                    &VType::from(FieldType::Ref(object_type)),
                )
            {
                return Err(VerifierErrorKind::InvalidType);
            }
        }

        Invoke(invoke_type, method_ref) => {
            let (class, is_interface, is_init, desc) =
                constants_reader.lookup_method(*method_ref)?;

            // Check that all the arguments match
            for expected_arg_type in desc.parameters.iter().rev() {
                let found_arg_type = pop_offset_vec(stack)?;
                let compatible = VerificationType::is_assignable(
                    &class_graph,
                    &found_arg_type,
                    &VType::from(expected_arg_type.clone()),
                );
                if !compatible {
                    log::error!(
                        "Incompatible argument types: found {:?} but expected {:?} (for {})",
                        found_arg_type,
                        expected_arg_type,
                        desc.render(),
                    );
                    return Err(VerifierErrorKind::InvalidType);
                }
            }

            if let (InvokeType::Special, true) = (invoke_type, is_init) {
                // Initialize
                match pop_offset_vec(stack)? {
                    UninitializedThis => {
                        replace_all(stack, &UninitializedThis, || Object(this_class.clone()));
                        replace_all(locals, &UninitializedThis, || Object(this_class.clone()));
                    }

                    Uninitialized((reftype, off)) => {
                        replace_all(stack, &Uninitialized((reftype.clone(), off)), || {
                            Object(reftype.clone())
                        });
                        replace_all(locals, &Uninitialized((reftype.clone(), off)), || {
                            Object(reftype.clone())
                        });
                    }

                    _ => return Err(VerifierErrorKind::InvalidType),
                }

                if is_interface || desc.return_type.is_some() {
                    return Err(VerifierErrorKind::InvalidType);
                }
            } else {
                let (is_interface2, needs_receiver) = match invoke_type {
                    InvokeType::Static => (false, false),
                    InvokeType::Virtual | InvokeType::Special => (false, true),
                    InvokeType::Interface(_) => (true, true),
                };

                if is_interface != is_interface2 {
                    return Err(VerifierErrorKind::InvalidType);
                }

                // Pop off the receiver type
                if needs_receiver {
                    let expected_reciever = constants_reader.lookup_class_reftype(class)?;
                    let found_reciever = pop_offset_vec(stack)?;
                    let compatible = VerificationType::is_assignable(
                        &class_graph,
                        &found_reciever,
                        &Object(expected_reciever),
                    );
                    if !compatible {
                        return Err(VerifierErrorKind::InvalidType);
                    }
                }

                // Push the return type
                if let Some(return_type) = desc.return_type {
                    stack.push(VType::from(return_type));
                }
            }
        }

        InvokeDynamic(invoke_dynamic_ref) => {
            let desc = constants_reader.lookup_invoke_dynamic(*invoke_dynamic_ref)?;

            // Check that all the arguments match
            for expected_arg_type in desc.parameters.iter().rev() {
                let found_arg_type = pop_offset_vec(stack)?;
                let compatible = VerificationType::is_assignable(
                    &class_graph,
                    &found_arg_type,
                    &VType::from(expected_arg_type.clone()),
                );
                if !compatible {
                    return Err(VerifierErrorKind::InvalidType);
                }
            }

            // Push the return type
            if let Some(return_type) = desc.return_type {
                stack.push(VType::from(return_type));
            }
        }

        New(cls_idx) => {
            let ref_type = constants_reader.lookup_class_reftype(*cls_idx)?;
            stack.push(Uninitialized((ref_type, insn_offset_in_basic_block)));
        }
        NewArray(base_type) => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            stack.push(Object(RefType::array(FieldType::Base(*base_type))));
        }
        ANewArray(cls_idx) => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            let ref_type = constants_reader.lookup_class_reftype(*cls_idx)?;
            stack.push(Object(RefType::array(FieldType::Ref(ref_type))));
        }
        ArrayLength => {
            let array_type = pop_offset_vec(stack)?;
            match array_type {
                Object(RefType::Array(_)) => (),
                _ => return Err(VerifierErrorKind::InvalidType),
            }
            stack.push(Integer);
        }

        CheckCast(target_cls_idx) => {
            match pop_offset_vec(stack)? {
                Object(_) => (),
                _ => return Err(VerifierErrorKind::InvalidType),
            }
            let ref_type = constants_reader.lookup_class_reftype(*target_cls_idx)?;
            stack.push(Object(ref_type));
        }
        InstanceOf(_) => {
            match pop_offset_vec(stack)? {
                Object(_) => (),
                _ => return Err(VerifierErrorKind::InvalidType),
            }
            stack.push(Integer);
        }
    }

    Ok(())
}

fn interpret_branch_instruction<Lbl, LblWide, LblNext>(
    frame: &mut Frame<RefType, (RefType, Offset)>,
    class_graph: &ClassGraph,
    this_method_return_type: &Option<FieldType>,
    insn: &BranchInstruction<Lbl, LblWide, LblNext>,
) -> Result<(), VerifierErrorKind> {
    use BranchInstruction::*;
    use VerificationType::*;

    type VType = VerificationType<RefType, (RefType, Offset)>;
    let Frame {
        ref mut stack,
        locals: _,
    } = frame;

    match insn {
        If(_, _, _) => pop_offset_vec_expecting_type(stack, Integer)?,
        IfICmp(_, _, _) => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            pop_offset_vec_expecting_type(stack, Integer)?;
        }
        IfACmp(_, _, _) => {
            let atype_1 = pop_offset_vec(stack)?;
            let atype_2 = pop_offset_vec(stack)?;
            if !atype_1.is_reference() || !atype_2.is_reference() {
                return Err(VerifierErrorKind::InvalidType);
            }
        }
        Goto(_) | GotoW(_) => (),
        TableSwitch { .. } => pop_offset_vec_expecting_type(stack, Integer)?,
        LookupSwitch { .. } => pop_offset_vec_expecting_type(stack, Integer)?,
        IReturn => {
            pop_offset_vec_expecting_type(stack, Integer)?;
            match *this_method_return_type {
                Some(FieldType::Base(BaseType::Int))
                | Some(FieldType::Base(BaseType::Char))
                | Some(FieldType::Base(BaseType::Short))
                | Some(FieldType::Base(BaseType::Byte))
                | Some(FieldType::Base(BaseType::Boolean)) => (),
                _ => return Err(VerifierErrorKind::InvalidType),
            }
        }
        LReturn => {
            pop_offset_vec_expecting_type(stack, Long)?;
            if *this_method_return_type != Some(FieldType::LONG) {
                return Err(VerifierErrorKind::InvalidType);
            }
        }
        FReturn => {
            pop_offset_vec_expecting_type(stack, Float)?;
            if *this_method_return_type != Some(FieldType::FLOAT) {
                return Err(VerifierErrorKind::InvalidType);
            }
        }
        DReturn => {
            pop_offset_vec_expecting_type(stack, Double)?;
            if *this_method_return_type != Some(FieldType::DOUBLE) {
                return Err(VerifierErrorKind::InvalidType);
            }
        }
        AReturn => {
            let atype = pop_offset_vec(stack)?;
            let is_compatible_return = if let Some(ret_type) = this_method_return_type {
                VerificationType::is_assignable(class_graph, &atype, &VType::from(ret_type.clone()))
            } else {
                false
            };
            if !is_compatible_return {
                return Err(VerifierErrorKind::InvalidType);
            }
        }
        Return => {
            if *this_method_return_type != None {
                return Err(VerifierErrorKind::InvalidType);
            }
        }
        AThrow => {
            let atype = pop_offset_vec(stack)?;
            let is_exception =
                VerificationType::is_assignable(class_graph, &atype, &Object(RefType::THROWABLE));
            if !is_exception {
                return Err(VerifierErrorKind::InvalidType);
            }
            stack.clear();
            stack.push(atype);
        }
        IfNull(_, _, _) => {
            let atype = pop_offset_vec(stack)?;
            if !atype.is_reference() {
                return Err(VerifierErrorKind::InvalidType);
            }
        }
        FallThrough(_) => (),
    }

    Ok(())
}

fn replace_all<C: Eq, U: Eq>(
    offset_vec: &mut OffsetVec<VerificationType<C, U>>,
    original: &VerificationType<C, U>,
    updated: impl Fn() -> VerificationType<C, U>,
) {
    let mut replaced: OffsetVec<VerificationType<C, U>> = std::mem::take(offset_vec)
        .into_iter()
        .map(|(_, _, ty)| if ty == *original { updated() } else { ty })
        .collect();

    std::mem::swap(offset_vec, &mut replaced);
}

fn get_local<C, U>(
    locals: &OffsetVec<VerificationType<C, U>>,
    offset: u16,
) -> Result<&VerificationType<C, U>, VerifierErrorKind> {
    locals
        .get_offset(Offset(offset as usize))
        .ok()
        .ok_or(VerifierErrorKind::InvalidIndex)
}

fn get_local_expecting_type<C: Eq, U: Eq>(
    locals: &OffsetVec<VerificationType<C, U>>,
    offset: u16,
    expected_type: VerificationType<C, U>,
) -> Result<(), VerifierErrorKind> {
    let typ = get_local(locals, offset)?;
    if *typ == expected_type {
        Ok(())
    } else {
        Err(VerifierErrorKind::InvalidType)
    }
}

fn update_local_type<C, U>(
    locals: &mut OffsetVec<VerificationType<C, U>>,
    offset: u16,
    new_type: VerificationType<C, U>,
) -> Result<(), VerifierErrorKind> {
    locals
        .set_offset(Offset(offset as usize), new_type)
        .ok()
        .ok_or(VerifierErrorKind::InvalidIndex)
        .map(|_| ())
}

fn pop_offset_vec<C, U>(
    stack: &mut OffsetVec<VerificationType<C, U>>,
) -> Result<VerificationType<C, U>, VerifierErrorKind> {
    stack
        .pop()
        .map(|(_, _, typ)| typ)
        .ok_or(VerifierErrorKind::EmptyStack)
}

fn pop_offset_vec_expecting_width<C, U>(
    stack: &mut OffsetVec<VerificationType<C, U>>,
    expected_width: usize,
) -> Result<VerificationType<C, U>, VerifierErrorKind> {
    let typ = stack
        .pop()
        .map(|(_, _, typ)| typ)
        .ok_or(VerifierErrorKind::EmptyStack)?;
    let found_width = typ.width();
    if found_width == expected_width {
        Ok(typ)
    } else {
        Err(VerifierErrorKind::InvalidWidth(found_width))
    }
}

fn pop_offset_vec_expecting_type<C: Eq, U: Eq>(
    stack: &mut OffsetVec<VerificationType<C, U>>,
    expected_type: VerificationType<C, U>,
) -> Result<(), VerifierErrorKind> {
    let typ = stack
        .pop()
        .map(|(_, _, typ)| typ)
        .ok_or(VerifierErrorKind::EmptyStack)?;
    if typ == expected_type {
        Ok(())
    } else {
        Err(VerifierErrorKind::InvalidType)
    }
}
