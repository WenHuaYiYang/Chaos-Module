//! UI-owner cache: unchanged secondary text must not restart layout/marquees.
pub(crate) struct TextCache<const ROWS:usize,const CAP:usize> {
    handles:[u32;ROWS],text:[[u8;CAP];ROWS],
}
impl<const ROWS:usize,const CAP:usize> TextCache<ROWS,CAP> {
    pub(crate) const EMPTY:Self=Self{handles:[0;ROWS],text:[[0;CAP];ROWS]};
    pub(crate) fn reset(&mut self){self.handles.fill(0);}
    /// `text` must point to a NUL-terminated string shorter than CAP.
    pub(crate) unsafe fn changed(&mut self,slot:usize,handle:u32,text:*const u8)->bool {
        if slot>=ROWS || CAP==0 || handle==0 || text.is_null(){return false;}
        let mut n=0;while n+1<CAP && *text.add(n)!=0{n+=1;}
        let value=core::slice::from_raw_parts(text,n);
        if self.handles[slot]==handle && self.text[slot][n]==0 && self.text[slot][..n]==*value{return false;}
        self.handles[slot]=handle;self.text[slot][..n].copy_from_slice(value);self.text[slot][n]=0;true
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn only_changed_text_or_new_page_updates(){unsafe{
        let mut c=TextCache::<2,32>::EMPTY;
        assert!(c.changed(0,10,b"Phone\0".as_ptr()));
        for _ in 0..200{assert!(!c.changed(0,10,b"Phone\0".as_ptr()));}
        assert!(c.changed(0,10,b"Ph\0".as_ptr()));
        assert!(c.changed(0,10,b"Phone\0".as_ptr()));
        assert!(c.changed(0,11,b"Phone\0".as_ptr()));
        assert!(!c.changed(0,0,b"Phone\0".as_ptr()));
        c.reset();assert!(c.changed(0,11,b"Phone\0".as_ptr()));
    }}
}
