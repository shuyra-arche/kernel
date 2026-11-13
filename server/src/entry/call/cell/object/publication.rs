use super::*;
use crate::basis::nest::memo::Publisher;

pub(super) fn publication(
    pos: CellPosition,
    publishing: bool,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let rights = if publishing {
        NestRights::PUBLISH
    } else {
        NestRights::RETRACT
    };
    with_mapping(pos, rights, ident, |actor, mapping, page| {
        let span = mapping.span().map_err(source_fail)?;
        if span.inspect_memo(page)?.generation != pos.generation {
            return Err(CellFail::Busy);
        }
        let (data, _) = mapping
            .nest()
            .association(&span, page)?
            .ok_or(CellFail::SourceEmpty)?;
        let change = crate::basis::cell::change::acquire(&data)?;
        if publishing {
            let publisher = {
                let _commit = crate::basis::commit();
                Publisher::new(actor, mapping.entry())?
            };
            mapping
                .nest()
                .publish_memo(&span, page, pos.generation, change, publisher)?;
        } else {
            mapping
                .nest()
                .retract_memo(&span, page, pos.generation, change)?;
        }
        Ok(0)
    })
}
