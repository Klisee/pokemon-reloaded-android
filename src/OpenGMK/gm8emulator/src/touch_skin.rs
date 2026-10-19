//! Bounded multi-row atlas, avoiding texture-size overflow with a full custom button set.
pub fn atlas_layout(sizes: &[(u32,u32)], limit:u32) -> Option<(u32,u32,Vec<(u32,u32)>)> {
    let mut positions=Vec::with_capacity(sizes.len());
    let(mut x,mut y,mut row_height,mut width)=(0u32,0u32,0u32,0u32);
    for &(w,h) in sizes {
        if w==0 || h==0 || w>limit || h>limit {return None}
        if x.checked_add(w)?>limit {y=y.checked_add(row_height)?.checked_add(1)?;x=0;row_height=0;}
        if y.checked_add(h)?>limit {return None}
        positions.push((x,y));width=width.max(x+w);x=x.checked_add(w)?.checked_add(1)?;row_height=row_height.max(h);
    }
    Some((width.max(1),(y+row_height).max(1),positions))
}
