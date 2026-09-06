macro_rules! ui {
    ($first:expr $(, $rest:expr)* $(,)?) => {
        embedded_layout::object_chain::Chain::new($first)$(.append($rest))*
    };
}

macro_rules! ui_column {
    ($gap:expr; $first:expr $(, $rest:expr)* $(,)?) => {
        embedded_layout::layout::linear::LinearLayout::vertical(
            embedded_layout::object_chain::Chain::new($first)$(.append($rest))*
        )
        .with_spacing(embedded_layout::layout::linear::FixedMargin($gap))
        .arrange()
    };
}

pub(crate) use {ui, ui_column};
