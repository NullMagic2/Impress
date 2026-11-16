"""Host transcription of test-support/ligature_font.rs, for table validation.

This constructs ORIGINAL rectangular test glyphs in memory, never reads a font
from the machine, and never writes a font file. It is a format/shaping oracle,
not execution of the Rust builder or the Impress editor.
"""
import struct


def u16s(*values):
    return bytearray(struct.pack('>' + 'H' * len(values), *(v & 0xffff for v in values)))


def p16(buf, at, value):
    struct.pack_into('>H', buf, at, value & 0xffff)


def p32(buf, at, value):
    struct.pack_into('>I', buf, at, value & 0xffffffff)


def checksum(buf):
    buf = bytes(buf) + b'\0' * (-len(buf) % 4)
    return sum(struct.unpack('>' + 'I' * (len(buf) // 4), buf)) & 0xffffffff


def lookup(first, ligatures):
    group = u16s(len(ligatures)) + bytearray(2 * len(ligatures))
    for index, (glyph, rest) in enumerate(ligatures):
        p16(group, 2 + 2 * index, len(group))
        group += u16s(glyph, len(rest) + 1, *rest)
    table = u16s(1, 8 + len(group), 1, 8) + group + u16s(1, 1, first)
    return u16s(4, 0, 1, 8) + table


def font():
    widths = [500,250,360,250,250,340,300,600,600,500,500,770,550,620,770]
    tables = {}
    head = bytearray(54)
    for at, v in [(0,0x10000),(4,0x10000),(12,0x5f0f3cf5)]: p32(head,at,v)
    for at, v in [(16,3),(18,1000),(40,800),(42,700),(46,8),(48,2),(50,1)]: p16(head,at,v)
    p32(head,24,3_655_000_000); p32(head,32,3_655_000_000)
    tables['head'] = head
    hhea = bytearray(36); p32(hhea,0,0x10000)
    for at,v in [(4,800),(6,-200),(10,800),(16,800),(18,1),(34,len(widths))]: p16(hhea,at,v)
    tables['hhea'] = hhea
    maxp = bytearray(32); p32(maxp,0,0x10000)
    for at,v in [(4,len(widths)),(6,4),(8,1),(14,1)]: p16(maxp,at,v)
    tables['maxp'] = maxp
    hmtx,glyf,loca = bytearray(),bytearray(),bytearray()
    for index,width in enumerate(widths):
        hmtx += u16s(width,0); loca += struct.pack('>I',len(glyf))
        if index == 1: continue
        w = width-100
        glyf += u16s(1,50,0,width-50,600,3,0) + bytes([1,1,1,1])
        glyf += u16s(50,w,0,-w,0,0,600,0)
        glyf += bytes(-len(glyf)%4)
    loca += struct.pack('>I',len(glyf))
    tables.update(hmtx=hmtx,glyf=glyf,loca=loca)
    mapping = [(32,1),(65,7),(86,8),(102,2),(105,3),(108,4),(115,5),(116,6),(0xffff,0)]
    n=len(mapping); power=1 << (n.bit_length()-1)
    sub=u16s(4,16+8*n,0,n*2,power*2,power.bit_length()-1,n*2-power*2)
    sub+=u16s(*(c for c,g in mapping))+u16s(0)+u16s(*(c for c,g in mapping))+u16s(*(g-c for c,g in mapping))+bytes(n*2)
    tables['cmap']=u16s(0,1,3,1)+struct.pack('>I',12)+sub
    names=[(1,'ImpressTest'),(2,'Regular'),(3,'ImpressTest-Regular'),(4,'ImpressTest Regular'),(6,'ImpressTest-Regular')]
    name=u16s(0,len(names),6+12*len(names));strings=bytearray()
    for id,text in names:
        text=text.encode('utf-16-be');name+=u16s(3,1,0x409,id,len(text),len(strings));strings+=text
    tables['name']=name+strings
    os2=bytearray(78);os2[58:62]=b'IMPR'
    for at,v in [(2,450),(4,400),(6,5),(62,0x40),(64,32),(66,116),(68,800),(70,-200),(74,800),(76,200)]: p16(os2,at,v)
    tables['OS/2']=os2
    post=bytearray(32);p32(post,0,0x30000);tables['post']=post
    scripts=u16s(1)+b'DFLT'+u16s(8,4,0,0,0xffff,2,0,1)
    features=u16s(2)+b'dlig'+u16s(14)+b'liga'+u16s(20)+u16s(0,1,1,0,1,0)
    standard=lookup(2,[(11,[2,3]),(14,[2,4]),(13,[2]),(9,[3]),(10,[4])]); discretionary=lookup(5,[(12,[6])])
    lookups=u16s(2,6,6+len(standard))+standard+discretionary
    tables['GSUB']=struct.pack('>I',0x10000)+u16s(10,10+len(scripts),10+len(scripts)+len(features))+scripts+features+lookups
    tables['kern']=u16s(0,1,0,20,1,1,6,0,0,7,8,-80)
    n=len(tables);power=1<<(n.bit_length()-1)
    out=bytearray(struct.pack('>I',0x10000))+u16s(n,power*16,power.bit_length()-1,n*16-power*16)+bytes(16*n)
    for i,(tag,table) in enumerate(sorted(tables.items())):
        at=12+i*16;out[at:at+4]=tag.encode('ascii');p32(out,at+4,checksum(table));p32(out,at+8,len(out));p32(out,at+12,len(table))
        if tag=='head': head_offset=len(out)
        out+=table;out+=bytes(-len(out)%4)
    p32(out,head_offset+8,0xb1b0afba-checksum(out))
    return bytes(out)
