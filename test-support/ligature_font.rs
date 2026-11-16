//! Builds a tiny ORIGINAL OpenType test face in memory. Rectangular glyphs only;
//! not a redistributable text font. No font file, system font or network is needed.
//! GSUB: ff/fi/fl/ffi/ffl in liga, st in dlig. kern: A V -80 units.
use std::collections::BTreeMap;
pub const FAMILY:&str="ImpressTest";
fn u16s(values:&[u16])->Vec<u8> { values.iter().flat_map(|v|v.to_be_bytes()).collect() }
fn put16(b:&mut [u8],at:usize,v:u16) { b[at..at+2].copy_from_slice(&v.to_be_bytes()); }
fn put32(b:&mut [u8],at:usize,v:u32) { b[at..at+4].copy_from_slice(&v.to_be_bytes()); }
fn checksum(bytes:&[u8])->u32 { bytes.chunks(4).fold(0u32,|sum,c| {let mut word=[0;4];word[..c.len()].copy_from_slice(c);sum.wrapping_add(u32::from_be_bytes(word))}) }
fn lookup(first:u16, ligatures:&[(u16,&[u16])])->Vec<u8> {
    let mut set=u16s(&[ligatures.len() as u16]);set.resize(2+2*ligatures.len(),0);
    for (index,(glyph,rest)) in ligatures.iter().enumerate() {
        let offset=set.len() as u16;put16(&mut set,2+2*index,offset);
        set.extend(u16s(&[*glyph,(rest.len()+1) as u16]));set.extend(u16s(rest));
    }
    let mut table=u16s(&[1,(8+set.len()) as u16,1,8]);table.extend(set);table.extend(u16s(&[1,1,first]));
    let mut lookup=u16s(&[4,0,1,8]);lookup.extend(table);lookup
}
pub fn font()->Vec<u8> {
    let widths=[500u16,250,360,250,250,340,300,600,600,500,500,770,550,620,770];
    let count=widths.len();
    let mut tables:BTreeMap<&str,Vec<u8>>=BTreeMap::new();
    let mut head=vec![0u8;54];put32(&mut head,0,0x10000);put32(&mut head,4,0x10000);put32(&mut head,12,0x5f0f3cf5);
    put16(&mut head,16,3);put16(&mut head,18,1000);put16(&mut head,40,800);put16(&mut head,42,700);put16(&mut head,46,8);put16(&mut head,48,2);put16(&mut head,50,1);
    put32(&mut head,24,3_655_000_000);put32(&mut head,32,3_655_000_000);
    tables.insert("head",head);
    let mut hhea=vec![0u8;36];put32(&mut hhea,0,0x10000);put16(&mut hhea,4,800);put16(&mut hhea,6,(-200i16) as u16);put16(&mut hhea,10,800);put16(&mut hhea,16,800);put16(&mut hhea,18,1);put16(&mut hhea,34,count as u16);tables.insert("hhea",hhea);
    let mut maxp=vec![0u8;32];put32(&mut maxp,0,0x10000);put16(&mut maxp,4,count as u16);put16(&mut maxp,6,4);put16(&mut maxp,8,1);put16(&mut maxp,14,1);tables.insert("maxp",maxp);
    let mut hmtx=Vec::new();let mut glyf=Vec::new();let mut loca=Vec::new();
    for (index,width) in widths.iter().enumerate() {
        hmtx.extend(u16s(&[*width,0]));loca.extend((glyf.len() as u32).to_be_bytes());
        if index==1 {continue;}
        let w=*width-100;
        glyf.extend(u16s(&[1,50,0,*width-50,600,3,0]));glyf.extend([1,1,1,1]);
        glyf.extend(u16s(&[50,w,0,(-(w as i16)) as u16,0,0,600,0]));
        while glyf.len()%4!=0 {glyf.push(0);}
    }
    loca.extend((glyf.len() as u32).to_be_bytes());
    tables.insert("hmtx",hmtx);tables.insert("glyf",glyf);tables.insert("loca",loca);
    let mapping=[(32u16,1u16),(65,7),(86,8),(102,2),(105,3),(108,4),(115,5),(116,6),(0xffff,0)];
    let segments=mapping.len() as u16;let power=1u16 << (15-segments.leading_zeros());
    let mut sub=u16s(&[4,16+8*segments,0,segments*2,power*2,power.ilog2() as u16,segments*2-power*2]);
    for (ch,_) in mapping {sub.extend(ch.to_be_bytes());}sub.extend([0,0]);
    for (ch,_) in mapping {sub.extend(ch.to_be_bytes());}
    for (ch,glyph) in mapping {sub.extend(glyph.wrapping_sub(ch).to_be_bytes());}
    sub.extend(vec![0;mapping.len()*2]);let mut cmap=u16s(&[0,1,3,1]);cmap.extend(12u32.to_be_bytes());cmap.extend(sub);tables.insert("cmap",cmap);
    let names=[(1,FAMILY),(2,"Regular"),(3,"ImpressTest-Regular"),(4,"ImpressTest Regular"),(6,"ImpressTest-Regular")];
    let mut name=u16s(&[0,names.len() as u16,6+12*names.len() as u16]);let mut strings=Vec::new();
    for (id,text) in names {
        let bytes:Vec<u8>=text.encode_utf16().flat_map(|c|c.to_be_bytes()).collect();
        name.extend(u16s(&[3,1,0x409,id,bytes.len() as u16,strings.len() as u16]));strings.extend(bytes);
    }name.extend(strings);tables.insert("name",name);
    let mut os2=vec![0u8;78];put16(&mut os2,2,450);put16(&mut os2,4,400);put16(&mut os2,6,5);os2[58..62].copy_from_slice(b"IMPR");put16(&mut os2,62,0x40);put16(&mut os2,64,32);put16(&mut os2,66,116);put16(&mut os2,68,800);put16(&mut os2,70,(-200i16) as u16);put16(&mut os2,74,800);put16(&mut os2,76,200);tables.insert("OS/2",os2);
    let mut post=vec![0u8;32];put32(&mut post,0,0x30000);tables.insert("post",post);
    let mut scripts=u16s(&[1]);scripts.extend(b"DFLT");scripts.extend(u16s(&[8,4,0,0,0xffff,2,0,1]));
    let mut features=u16s(&[2]);features.extend(b"dlig");features.extend(u16s(&[14]));features.extend(b"liga");features.extend(u16s(&[20]));features.extend(u16s(&[0,1,1,0,1,0]));
    let standard=lookup(2,&[(11,&[2,3]),(14,&[2,4]),(13,&[2]),(9,&[3]),(10,&[4])]);let discretionary=lookup(5,&[(12,&[6])]);
    let mut lookups=u16s(&[2,6,(6+standard.len()) as u16]);lookups.extend(standard);lookups.extend(discretionary);
    let mut gsub=0x10000u32.to_be_bytes().to_vec();gsub.extend(u16s(&[10,(10+scripts.len()) as u16,(10+scripts.len()+features.len()) as u16]));gsub.extend(scripts);gsub.extend(features);gsub.extend(lookups);tables.insert("GSUB",gsub);
    tables.insert("kern",u16s(&[0,1,0,20,1,1,6,0,0,7,8,(-80i16) as u16]));
    let n=tables.len();let pow=1usize << n.ilog2();
    let mut file=0x10000u32.to_be_bytes().to_vec();file.extend(u16s(&[n as u16,(pow*16) as u16,pow.ilog2() as u16,(n*16-pow*16) as u16]));file.resize(12+16*n,0);
    let mut head_offset=0;
    for (index,(tag,table)) in tables.into_iter().enumerate() {
        let entry=12+16*index;file[entry..entry+4].copy_from_slice(tag.as_bytes());
        let offset=file.len();put32(&mut file,entry+4,checksum(&table));put32(&mut file,entry+8,offset as u32);put32(&mut file,entry+12,table.len() as u32);
        if tag=="head" {head_offset=offset;}
        file.extend(table);while file.len()%4!=0 {file.push(0);}
    }
    let adjustment=0xb1b0afbau32.wrapping_sub(checksum(&file));put32(&mut file,head_offset+8,adjustment);file
}
