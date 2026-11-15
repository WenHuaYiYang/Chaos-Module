//! Local manager paths: bounded joins, UTF-8 names, no traversal or device reads.
pub const CAP:usize=256;
pub fn len(p:&[u8])->usize{p.iter().position(|&c|c==0).unwrap_or(p.len())}
pub fn join(dir:&[u8],name:&[u8])->Option<[u8;CAP]>{
    let d=&dir[..len(dir)];let n=&name[..len(name)];
    if d.first()!=Some(&b'/')||n.is_empty()||n==b"."||n==b".."||n.iter().any(|&c|c==b'/'||c<32)||core::str::from_utf8(n).is_err(){return None;}
    let slash=usize::from(d!=b"/");if d.len()+slash+n.len()>=CAP{return None;}
    let mut out=[0;CAP];out[..d.len()].copy_from_slice(d);let mut at=d.len();
    if slash!=0{out[at]=b'/';at+=1;}out[at..at+n.len()].copy_from_slice(n);Some(out)
}
pub fn parent(path:&[u8])->[u8;CAP]{
    let p=&path[..len(path)];let at=p.iter().rposition(|&c|c==b'/').unwrap_or(0).max(1);
    let mut out=[0;CAP];out[..at].copy_from_slice(&p[..at]);out
}
pub fn blocked(path:&[u8])->bool{
    let p=&path[..len(path)];[b"/dev".as_slice(),b"/proc",b"/sys"].iter().any(|r|p==*r||(p.starts_with(r)&&p.get(r.len())==Some(&b'/')))
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn paths_preserve_names_and_reject_escape(){
  assert_eq!(&join(b"/",b"data").unwrap()[..6],b"/data\0");
  let p=join(b"/data\0", "说明.txt".as_bytes()).unwrap();assert_eq!(len(&p),"/data/说明.txt".len());
  assert_eq!(&parent(&p)[..6],b"/data\0");assert_eq!(&parent(b"/")[..2],b"/\0");
  for n in [b"..".as_slice(),b"a/b",b"",b"x\ny",b"\xff"]{assert!(join(b"/data",n).is_none());}
  assert!(join(&[b'x';250],b"longname").is_none());
  assert!(blocked(b"/dev/tty0"));assert!(blocked(b"/proc"));assert!(!blocked(b"/data/dev"));
 }
}
