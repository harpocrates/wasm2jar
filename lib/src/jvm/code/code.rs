use crate::jvm::class_file;
use crate::jvm::class_file::{BytecodeIndex, ClassConstantIndex, ConstantsPool, Serialize};
use crate::jvm::class_graph::BootstrapMethodId;
use crate::jvm::code::{
    jump_encoding, SerializableBasicBlock, SynLabel, SynLabelGenerator, VerifierBasicBlock,
};
use crate::jvm::verifier::Frame;
use crate::jvm::Error;
use crate::util::{Offset, Width};
use std::collections::{HashMap, HashSet};

/// Semantic representation of a method body
pub struct Code<'g> {
    /// Maximum size of locals through the method
    pub max_locals: Offset,

    /// Maximum size of stack through the method
    pub max_stack: Offset,

    /// Basic blocks in the code
    pub blocks: HashMap<SynLabel, VerifierBasicBlock<'g>>,

    /// Order of basic blocks in the code (elements are unique and exactly match keys of `blocks`)
    pub block_order: Vec<SynLabel>,

    /// Generator to produce the next label
    pub label_generator: SynLabelGenerator,
}

impl<'g> Code<'g> {
    pub fn serialize_code(
        mut self,
        constants_pool: &mut ConstantsPool<'g>,
        bootstrap_methods: &mut HashMap<BootstrapMethodId<'g>, u16>,
    ) -> Result<class_file::Code, Error> {
        // Convert max locals and stack
        let max_locals: u16 = match u16::try_from(self.max_locals.0) {
            Ok(max_locals) => max_locals,
            Err(_) => return Err(Error::MethodCodeMaxStackOverflow(self.max_locals)),
        };
        let max_stack: u16 = match u16::try_from(self.max_stack.0) {
            Ok(max_stack) => max_stack,
            Err(_) => return Err(Error::MethodCodeMaxStackOverflow(self.max_stack)),
        };

        // Extract a mapping of label to offset and labels used (our next iteration is destructive)
        let mut blocks: HashMap<SynLabel, SerializableBasicBlock> = HashMap::new();
        let mut latest_offset = Offset(0);
        for block_label in &self.block_order {
            let block = self.blocks.remove(block_label).expect("Missing block");
            let block: SerializableBasicBlock =
                block.serialize_instructions(constants_pool, bootstrap_methods, latest_offset)?;
            latest_offset.0 += block.width();
            blocks.insert(*block_label, block);
        }

        // Check and rewrite oversized jumps
        jump_encoding::widen_oversized_jumps(
            &mut self.block_order,
            &mut blocks,
            &mut self.label_generator,
            &jump_encoding::SIGNED_16BIT_JUMP_RANGE,
        );

        // Extract a mapping of label to offset and labels used
        let mut label_offsets: HashMap<SynLabel, Offset> = HashMap::new();
        let mut jump_targets: HashSet<SynLabel> = HashSet::new();
        let mut latest_offset = Offset(0);
        for block_label in &self.block_order {
            let block = &blocks[&block_label];
            label_offsets.insert(*block_label, latest_offset);
            jump_targets.extend(block.branch_end.jump_targets().targets());
            latest_offset.0 += block.width();
        }

        // Check if we've got an overflow
        if let Err(_) = u16::try_from(latest_offset.0) {
            return Err(Error::MethodCodeOverflow(latest_offset));
        }

        // Loop through the blocks in placement order to accumulate code and frames
        let mut code_array: class_file::BytecodeArray = class_file::BytecodeArray(vec![]);
        let implicit_frame: Frame<ClassConstantIndex, BytecodeIndex> = blocks[&SynLabel::START]
            .frame
            .into_serializable(constants_pool, &label_offsets)?;
        let mut frames: Vec<(Offset, Frame<ClassConstantIndex, BytecodeIndex>)> = vec![];
        let mut fallthrough_label: Option<SynLabel> = None;

        for block_label in &self.block_order {
            if let Some(fallthrough_label) = fallthrough_label.take() {
                assert_eq!(
                    fallthrough_label, *block_label,
                    "fallthrough does not match next block"
                );
            }
            let basic_block = blocks
                .remove(block_label)
                .expect("No such block or block already placed");
            let block_offset_from_start = label_offsets[block_label];

            // If this block is ever jumped to, construct a stack map frame for it
            if jump_targets.contains(&block_label) {
                frames.push((
                    block_offset_from_start,
                    basic_block
                        .frame
                        .into_serializable(constants_pool, &label_offsets)?,
                ));
            }

            // Serialize the instructions in the block to the bytecode array
            for (_, _, insn) in basic_block.instructions.iter() {
                insn.serialize(&mut code_array.0).map_err(Error::IoError)?;
            }
            let branch_end_offset: i64 =
                (block_offset_from_start.0 + basic_block.instructions.offset_len().0) as i64;
            let end_insn = basic_block.branch_end.map_labels(
                |lbl: &SynLabel| {
                    i16::try_from(label_offsets[lbl].0 as i64 - branch_end_offset)
                        .expect("regular jump overflow")
                },
                |lbl: &SynLabel| {
                    i32::try_from(label_offsets[lbl].0 as i64 - branch_end_offset)
                        .expect("wide jump overflow")
                },
                |_| (),
            );
            end_insn
                .serialize(&mut code_array.0)
                .map_err(Error::IoError)?;

            fallthrough_label = basic_block.branch_end.fallthrough_target();
        }
        assert_eq!(
            fallthrough_label, None,
            "method cannot end with fallthrough label"
        );

        // Build up stack map frames
        let mut previous_frame = implicit_frame;
        let mut previous_offset = Offset(0);
        let mut stack_map_frames = vec![];
        for (offset, frame) in frames {
            let offset_delta = if stack_map_frames.is_empty() {
                offset.0 - previous_offset.0
            } else if offset == previous_offset {
                if frame != previous_frame {
                    return Err(Error::ConflictingFrames(offset, frame, previous_frame));
                } else {
                    continue;
                }
            } else {
                offset.0 - previous_offset.0 - 1
            };
            stack_map_frames.push(frame.stack_map_frame(offset_delta as u16, &previous_frame));

            previous_frame = frame;
            previous_offset = offset;
        }

        let mut attributes = vec![];

        // Add `StackMapTable` attribute only if there are frames
        if !stack_map_frames.is_empty() {
            let stack_map_table = class_file::StackMapTable(stack_map_frames);
            attributes.push(constants_pool.get_attribute(stack_map_table)?);
        }

        Ok(class_file::Code {
            max_stack,
            max_locals,
            code_array,
            exception_table: vec![],
            attributes,
        })
    }
}
