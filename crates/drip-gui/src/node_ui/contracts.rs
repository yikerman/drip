//! Coordinate checks shared by GUI image consumers.
use drip::{Error, Result, node::data::*};
pub fn working(image: Option<&ImageDesc<Color>>) -> Result<()> {
    if image.is_some_and(|d| d.interpretation != drip::node::raw::working_color()) {
        return Err(Error::Contract(
            "image must use identity-encoded Rec.2020/D65 with relative white 1".into(),
        ));
    }
    Ok(())
}
