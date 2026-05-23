interstice_module!(visibility: Private, authorities: [Gpu]);

pub use crate::bindings::graphics::*;
use interstice_sdk::*;

#[table]
pub struct ElementSchema {
    #[primary_key(auto_inc)]
    id: u64,
    parent: Option<u64>,
    children: Vec<u64>,
    x: u32,
    y: u32,
    width: Size,
    height: Size,
    filled: bool,
    stroke_width: u32,
    background_color: (f32, f32, f32, f32),
    margin: u32,
    padding: u32,
    layout_direction: LayoutDirection,
}

pub struct Element {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    filled: bool,
    stroke_width: u32,
    background_color: (f32, f32, f32, f32),
}

#[interstice_type]
pub enum LayoutDirection {
    LeftToRight,
    TopToBottom,
    RightToLeft,
    BottomToTop,
}

pub struct SurfaceInfos {
    width: u32,
    height: u32,
}

#[interstice_type]
pub enum Position {
    Fixed(u32),
    Auto,
}

#[interstice_type]
pub enum Size {
    Fixed(u32),
    Grow,
    Fit,
}

fn compute_elements<Caps>(
    ctx: &ReducerContext<Caps>,
    element_schema: ElementSchema,
    mut left_offset: u32,
    mut top_offset: u32,
    mut max_width: u32,
    mut max_height: u32,
) -> Vec<Element>
where
    Caps: CanRead<ElementSchema>,
{
    let x = left_offset + element_schema.x + element_schema.margin;
    let y = top_offset + element_schema.y + element_schema.margin;
    let mut width = match element_schema.width {
        Size::Fixed(px) => px,
        Size::Grow => max_width - element_schema.margin * 2,
        Size::Fit => 0, // Will be computed based on content
    };
    let mut height = match element_schema.height {
        Size::Fixed(px) => px,
        Size::Grow => max_height,
        Size::Fit => 0, // Will be computed based on content
    };
    left_offset = x + element_schema.padding;
    top_offset = y + element_schema.padding;
    max_width = match element_schema.width {
        Size::Fixed(px) => px,
        Size::Grow => max_width - element_schema.padding * 2 - element_schema.margin * 2,
        Size::Fit => max_width - element_schema.padding * 2 - element_schema.margin * 2,
    };
    max_height = match element_schema.height {
        Size::Fixed(px) => px,
        Size::Grow => max_height - element_schema.padding * 2 - element_schema.margin * 2,
        Size::Fit => max_height - element_schema.padding * 2 - element_schema.margin * 2,
    };

    let mut elements = Vec::new();
    let mut childs_elements = Vec::new();
    for child_id in element_schema.children {
        let child_schema = ctx.current.tables.elementschema().get(child_id).unwrap();
        let child_elements = compute_elements(
            ctx,
            child_schema,
            left_offset,
            top_offset,
            max_width,
            max_height,
        );
        let child_element = &child_elements[0];
        if let Size::Fit = element_schema.width {
            if let LayoutDirection::LeftToRight | LayoutDirection::RightToLeft =
                element_schema.layout_direction
            {
                width += child_element.width;
            } else {
                width = width.max(child_element.width);
            }
        }
        if let Size::Fit = element_schema.height {
            if let LayoutDirection::TopToBottom | LayoutDirection::BottomToTop =
                element_schema.layout_direction
            {
                height += child_element.height;
            } else {
                height = height.max(child_element.height);
            }
        }
        match element_schema.layout_direction {
            LayoutDirection::LeftToRight => left_offset += child_element.width,
            LayoutDirection::TopToBottom => top_offset += child_element.height,
            LayoutDirection::RightToLeft => left_offset -= child_element.width,
            LayoutDirection::BottomToTop => top_offset -= child_element.height,
        }
        childs_elements.extend(child_elements);
    }

    elements.push(Element {
        x,
        y,
        width,
        height,
        filled: element_schema.filled,
        stroke_width: element_schema.stroke_width,
        background_color: element_schema.background_color,
    });
    elements.extend(childs_elements);

    return elements;
}

fn draw_ui<Caps>(ctx: ReducerContext<Caps>, surface_infos: SurfaceInfos)
where
    Caps: CanRead<ElementSchema>,
{
    let graphics = ctx.graphics();
    let schemas = ctx.current.tables.elementschema().scan();
    let root_nodes = schemas.into_iter().filter(|e| e.parent.is_none());

    for root_node in root_nodes {
        let layer = format!("ui_{}", root_node.id);
        let elements = compute_elements(
            &ctx,
            root_node,
            0,
            0,
            surface_infos.width,
            surface_infos.height,
        );

        for element in elements {
            let x = element.x;
            let y = element.y;
            let w = element.width;
            let h = element.height;
            let (r, g, b, a) = element.background_color;

            graphics
                .reducers
                .draw_rect(
                    layer.clone(),
                    Rect { x, y, w, h },
                    Color { r, g, b, a },
                    element.filled,
                    element.stroke_width,
                )
                .unwrap();
        }
    }
}
