//! Bounded screenshot codecs; no firmware calls, heap allocator or UI objects.
//! Output is BGR888 for the verified native RGB888 image descriptor.
use miniz_oxide::inflate::{core::{decompress, DecompressorOxide, inflate_flags}, TINFLStatus};
pub const MAX_PIXELS: usize = 336 * 480;
pub const MAX_FILE: usize = 2 * 1024 * 1024;
#[derive(Clone, Copy, Debug)]
pub struct Layout { pub width: usize, pub height: usize, pub raw: usize }
fn le(data: &[u8], i: usize) -> Result<u32, i32> {
    Ok(u32::from_le_bytes(data.get(i..i+4).ok_or(-22)?.try_into().map_err(|_|-22)?))
}
fn be(data: &[u8], i: usize) -> Result<u32, i32> {
    Ok(u32::from_be_bytes(data.get(i..i+4).ok_or(-22)?.try_into().map_err(|_|-22)?))
}
fn dimensions(width: usize, height: usize, raw: usize) -> Result<Layout, i32> {
    if width == 0 || height == 0 || width > 1024 || height > 1024 || width*height > MAX_PIXELS { return Err(-95); }
    Ok(Layout {width,height,raw})
}
pub fn inspect(data: &[u8]) -> Result<Layout,i32> {
    if data.len() > MAX_FILE {return Err(-27);}
    if data.starts_with(b"BM") {
        let width=le(data,18)? as i32; let height=le(data,22)? as i32;
        if width <= 0 || height == i32::MIN {return Err(-22);}
        return dimensions(width as usize,height.unsigned_abs() as usize,0);
    }
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") || be(data,8)? != 13 || data.get(12..16) != Some(b"IHDR") {return Err(-95);}
    let width=be(data,16)? as usize;let height=be(data,20)? as usize;
    let header=data.get(24..29).ok_or(-22)?;
    let channels=match header[1]{0|3=>1,2=>3,4=>2,6=>4,_=>return Err(-95)};
    if header[0]!=8 || header[2..]!=[0,0,0] {return Err(-95);}
    let layout=dimensions(width,height,0)?;
    Ok(Layout{raw:(layout.width*channels+1)*layout.height,..layout})
}
pub fn fit(layout: Layout, width: usize, height: usize) -> (usize,usize) {
    if layout.width*height > layout.height*width { (width,(layout.height*width/layout.width).max(1)) }
    else { ((layout.width*height/layout.height).max(1),height) }
}
const fn crc_table()->[u32;256] {
    let mut table=[0;256];let mut i=0;
    while i<256 {let mut v=i as u32;let mut j=0;while j<8{v=(v>>1)^(0xedb88320u32&0u32.wrapping_sub(v&1));j+=1;}table[i]=v;i+=1;}table
}
const CRC:[u32;256]=crc_table();
fn crc(bytes:&[u8])->u32 {
    let mut c=u32::MAX;for &b in bytes {c=CRC[((c^b as u32)&255)as usize]^(c>>8);}!c
}
fn paeth(a:u8,b:u8,c:u8)->u8 {
    let p=a as i32+b as i32-c as i32;
    let (pa,pb,pc)=((p-a as i32).abs(),(p-b as i32).abs(),(p-c as i32).abs());
    if pa<=pb&&pa<=pc{a}else if pb<=pc{b}else{c}
}
/// PNG filters 0..4; prior rows are already reconstructed in the same buffer.
fn unfilter(raw:&mut[u8],width:usize,height:usize,channels:usize)->Result<(),i32> {
    let stride=width*channels+1;
    for y in 0..height {
        let base=y*stride;let filter=raw[base];if filter>4{return Err(-22);}
        for x in 0..width*channels {
            let i=base+1+x;
            let a=if x>=channels{raw[i-channels]}else{0};
            let b=if y>0{raw[i-stride]}else{0};
            let c=if y>0&&x>=channels{raw[i-stride-channels]}else{0};
            let predict=match filter{0=>0,1=>a,2=>b,3=>((a as u16+b as u16)/2)as u8,_=>paeth(a,b,c)};
            raw[i]=raw[i].wrapping_add(predict);
        }
    } Ok(())
}
/// Downsample with area averaging, preserving thin text better than nearest neighbour.
fn sample(out:&mut[u8],dw:usize,dh:usize,sw:usize,sh:usize,pixel:impl Fn(usize,usize)->[u8;3]) {
    for y in 0..dh {for x in 0..dw {
        let (left,right)=(x*sw/dw,((x+1)*sw/dw).max(x*sw/dw+1).min(sw));
        let (top,bottom)=(y*sh/dh,((y+1)*sh/dh).max(y*sh/dh+1).min(sh));
        let mut sum=[0u32;3];let n=((right-left)*(bottom-top))as u32;
        for sy in top..bottom{for sx in left..right{let p=pixel(sx,sy);for c in 0..3{sum[c]+=p[c]as u32;}}}
        for c in 0..3{out[(y*dw+x)*3+c]=((sum[c]+n/2)/n)as u8;}
    }}
}
pub fn decode(data:&mut[u8],raw:&mut[u8],out:&mut[u8],dw:usize,dh:usize)->Result<(),i32> {
    let info=inspect(data)?;
    if dw==0||dh==0||dw>336||dh>480||out.len()!=dw*dh*3||raw.len()!=info.raw{return Err(-22);}
    if info.raw==0 {
        let dib=le(data,14)? as usize;let offset=le(data,10)? as usize;
        if dib<40 || dib>data.len()-14 || offset<14+dib || data.get(26..28)!=Some(&[1,0]) || le(data,30)?!=0{return Err(-95);}
        let bits=u16::from_le_bytes(data.get(28..30).ok_or(-22)?.try_into().map_err(|_|-22)?);
        let channels=match bits{24=>3,32=>4,_=>return Err(-95)};
        let stride=(info.width*channels+3)&!3;
        if offset.checked_add(stride*info.height).ok_or(-22)?>data.len(){return Err(-22);}
        let top_down=(le(data,22)?as i32)<0;
        sample(out,dw,dh,info.width,info.height,|x,y|{
            let row=if top_down{y}else{info.height-1-y};let i=offset+row*stride+x*channels;
            [data[i],data[i+1],data[i+2]]
        });return Ok(());
    }
    let color=data[25];let channels=match color{0|3=>1,2=>3,4=>2,6=>4,_=>return Err(-95)};
    let mut palette=[0u8;768];let mut alpha=[255u8;256];let mut palette_len=0;
    let mut transparent:Option<[u8;3]>=None;
    let mut offset=8;let mut packed=0;let mut idat=false;let mut idat_end=false;let mut end=false;let mut seen_palette=false;let mut seen_alpha=false;
    while offset<data.len() {
        let n=be(data,offset)? as usize;let chunk_end=offset.checked_add(12).and_then(|v|v.checked_add(n)).ok_or(-22)?;
        if chunk_end>data.len(){return Err(-22);}
        let tag:&[u8]=&data[offset+4..offset+8];let start=offset+8;let stop=start+n;
        if crc(&data[offset+4..stop])!=be(data,stop)?{return Err(-22);}
        match tag {
            b"IHDR"=>{if offset!=8||n!=13{return Err(-22);}},
            b"PLTE"=>{
                if idat||seen_palette||n==0||n>768||n%3!=0{return Err(-22);}
                palette[..n].copy_from_slice(&data[start..stop]);palette_len=n/3;seen_palette=true;
            },
            b"tRNS"=>{
                if idat||seen_alpha{return Err(-22);}seen_alpha=true;
                match color {
                    3 if seen_palette&&n<=palette_len=>alpha[..n].copy_from_slice(&data[start..stop]),
                    0 if n==2&&data[start]==0=>transparent=Some([data[start+1];3]),
                    2 if n==6&&data[start]==0&&data[start+2]==0&&data[start+4]==0=>transparent=Some([data[start+1],data[start+3],data[start+5]]),
                    _=>return Err(-95),
                }
            },
            b"IDAT"=>{
                if idat_end||color==3&&!seen_palette{return Err(-22);}
                idat=true;data.copy_within(start..stop,packed);packed+=n;
            },
            b"IEND"=>{if n!=0||!idat||chunk_end!=data.len(){return Err(-22);}end=true;break;},
            _=>{if tag[0]&32==0{return Err(-95);}if idat{idat_end=true;}},
        }
        offset=chunk_end;
    }
    if !end||packed==0{return Err(-22);}
    let mut inflater=DecompressorOxide::new();
    let flags=inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER|inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let(status,consumed,written)=decompress(&mut inflater,&data[..packed],raw,0,flags);
    if status!=TINFLStatus::Done||consumed!=packed||written!=raw.len(){return Err(-22);}
    unfilter(raw,info.width,info.height,channels)?;
    if color==3&&raw.chunks_exact(info.width+1).any(|r|r[1..].iter().any(|&i|i as usize>=palette_len)){return Err(-22);}
    sample(out,dw,dh,info.width,info.height,|x,y|{
        let i=y*(info.width*channels+1)+1+x*channels;
        let (r,g,b,a)=match color {
            0=>(raw[i],raw[i],raw[i],255),2=>(raw[i],raw[i+1],raw[i+2],255),
            3=>{let v=raw[i]as usize;(palette[v*3],palette[v*3+1],palette[v*3+2],alpha[v])},
            4=>(raw[i],raw[i],raw[i],raw[i+1]),_ =>(raw[i],raw[i+1],raw[i+2],raw[i+3]),
        };
        let a=if transparent==Some([r,g,b]){0}else{a as u32};
        [((b as u32*a+127)/255)as u8,((g as u32*a+127)/255)as u8,((r as u32*a+127)/255)as u8]
    });Ok(())
}
