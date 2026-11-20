#![allow(dead_code)]
#[path="../src/capture_decode.rs"] mod decode;
#[path="../src/capture_format.rs"] mod capture_format;
#[path="../src/capture_apng.rs"] mod capture_apng;
struct Sink(Vec<u8>);
impl capture_format::Sink for Sink {fn put(&mut self,data:&[u8])->Result<(),i32>{self.0.extend_from_slice(data);Ok(())}}
fn main(){
    let a:Vec<_>=std::env::args().collect();
    let mut data=std::fs::read(&a[2]).unwrap();
    if a[1]=="encode" {let mut out=Sink(vec![]);capture_apng::png(&mut out,&data).unwrap();std::fs::write(&a[3],out.0).unwrap();return;}
    let info=decode::inspect(&data);
    if a[1]=="reject" {
        if let Ok(info)=info {
            let mut raw=vec![0;info.raw];let mut out=vec![0;info.width*info.height*3];
            assert!(decode::decode(&mut data,&mut raw,&mut out,info.width,info.height).is_err(),"accepted malformed image: {}",a[2]);
        }return;
    }
    let info=info.unwrap();let w=a[4].parse().unwrap();let h=a[5].parse().unwrap();
    let(w,h)=decode::fit(info,w,h);let mut raw=vec![0;info.raw];let mut out=vec![0;w*h*3];
    decode::decode(&mut data,&mut raw,&mut out,w,h).unwrap();
    let expected=std::fs::read(&a[3]).unwrap();assert_eq!(out.len(),expected.len());
    assert!(out==expected,"pixel mismatch: {}",a[2]);
}
