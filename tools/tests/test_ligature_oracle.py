#!/usr/bin/env python3
"""Real HarfBuzz tests on an ORIGINAL in-memory fixture; NOT Rust/editor tests.

Optional developer test: needs fontTools and an installed HarfBuzz shared
library. No font files are read, generated on disk, or distributed. Rust tests
in the actual crates must still be executed with check.bat on the build host.
"""
import ctypes as C
import ctypes.util
import io
import unittest
import support  # noqa: F401  (puts tools/ on the import path)
try:
    from fontTools.ttLib import TTFont
    from ligature_fixture_oracle import font, checksum
except ModuleNotFoundError as missing:
    raise unittest.SkipTest(f'optional check: {missing.name} is not installed')

class Info(C.Structure):
    _fields_=[('gid',C.c_uint32),('mask',C.c_uint32),('cluster',C.c_uint32),('v1',C.c_uint32),('v2',C.c_uint32)]
class Position(C.Structure):
    _fields_=[('xa',C.c_int32),('ya',C.c_int32),('xo',C.c_int32),('yo',C.c_int32),('v',C.c_uint32)]
class Feature(C.Structure):
    _fields_=[('tag',C.c_uint32),('value',C.c_uint32),('start',C.c_uint),('end',C.c_uint)]


def library():
    name=ctypes.util.find_library('harfbuzz')
    if not name: raise unittest.SkipTest('HarfBuzz shared library is not installed')
    lib=C.CDLL(name)
    prototypes={
        'hb_blob_create':([C.c_void_p,C.c_uint,C.c_int,C.c_void_p,C.c_void_p],C.c_void_p),
        'hb_face_create':([C.c_void_p,C.c_uint],C.c_void_p),
        'hb_font_create':([C.c_void_p],C.c_void_p),
        'hb_ot_font_set_funcs':([C.c_void_p],None),
        'hb_font_set_scale':([C.c_void_p,C.c_int,C.c_int],None),
        'hb_buffer_create':([],C.c_void_p),
        'hb_buffer_add_utf8':([C.c_void_p,C.c_char_p,C.c_int,C.c_uint,C.c_int],None),
        'hb_buffer_guess_segment_properties':([C.c_void_p],None),
        'hb_buffer_set_cluster_level':([C.c_void_p,C.c_uint],None),
        'hb_shape':([C.c_void_p,C.c_void_p,C.POINTER(Feature),C.c_uint],None),
        'hb_buffer_get_glyph_infos':([C.c_void_p,C.POINTER(C.c_uint)],C.POINTER(Info)),
        'hb_buffer_get_glyph_positions':([C.c_void_p,C.POINTER(C.c_uint)],C.POINTER(Position)),
    }
    for resource in ['blob','face','font','buffer']:
        prototypes['hb_'+resource+'_destroy']=([C.c_void_p],None)
    for name,(args,ret) in prototypes.items():
        fn=getattr(lib,name);fn.argtypes=args;fn.restype=ret
    return lib


class LigatureOracleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data=font();cls.hb=library()

    def shape(self,text,liga=1,dlig=0,kern=1):
        hb=self.hb;memory=C.create_string_buffer(self.data)
        blob=hb.hb_blob_create(memory,len(self.data),0,None,None)
        face=hb.hb_face_create(blob,0);f=hb.hb_font_create(face);buffer=hb.hb_buffer_create()
        try:
            hb.hb_ot_font_set_funcs(f);hb.hb_font_set_scale(f,1000,1000)
            encoded=text.encode();hb.hb_buffer_add_utf8(buffer,encoded,len(encoded),0,len(encoded))
            hb.hb_buffer_set_cluster_level(buffer,0);hb.hb_buffer_guess_segment_properties(buffer)
            features=(Feature*3)(*(Feature(int.from_bytes(tag.encode(),'big'),v,0,0xffffffff) for tag,v in [('liga',liga),('dlig',dlig),('kern',kern)]))
            hb.hb_shape(f,buffer,features,3);n=C.c_uint()
            info=hb.hb_buffer_get_glyph_infos(buffer,C.byref(n));pos=hb.hb_buffer_get_glyph_positions(buffer,C.byref(n))
            return [(info[i].gid,info[i].cluster,pos[i].xa) for i in range(n.value)]
        finally:
            hb.hb_buffer_destroy(buffer);hb.hb_font_destroy(f);hb.hb_face_destroy(face);hb.hb_blob_destroy(blob)

    def test_ff_and_ffl_are_distinct_font_substitutions(self):
        for text, glyph in [('ff',13),('ffl',14)]:
            result = self.shape(text)
            self.assertEqual(len(result),1)
            self.assertEqual(result[0][0],glyph)
            self.assertEqual(len(self.shape(text,liga=0)),len(text))

    def test_absent_ff_is_not_invented_when_only_fi_fl_are_available(self):
        face=TTFont(io.BytesIO(self.data))
        substitutions=face['GSUB'].table.LookupList.Lookup[0].SubTable[0].ligatures
        substitutions['f']=[rule for rule in substitutions['f'] if rule.Component in [['i'],['l']]]
        buffer=io.BytesIO();face.save(buffer);original=self.data;self.data=buffer.getvalue()
        try:
            self.assertEqual(len(self.shape('ff')),2)
            self.assertEqual(len(self.shape('ffl')),2)  # f + fl is NOT an ffl ligature.
            self.assertEqual(len(self.shape('fi')),1)
            self.assertEqual(len(self.shape('fl')),1)
        finally:
            self.data=original

    def test_tables_parse_and_checksums_match(self):
        self.assertEqual(checksum(self.data),0xb1b0afba)
        face=TTFont(io.BytesIO(self.data),checkChecksums=2)
        self.assertEqual(face['head'].unitsPerEm,1000)
        self.assertEqual(face['maxp'].numGlyphs,15)
        self.assertEqual(face['name'].getDebugName(1),'ImpressTest')
        self.assertEqual({r.FeatureTag for r in face['GSUB'].table.FeatureList.FeatureRecord},{'liga','dlig'})
        for name in face.getGlyphOrder():
            if name!='space': self.assertEqual(face['glyf'][name].numberOfContours,1)

    def test_standard_ligatures_and_original_clusters(self):
        for text,gid,width in [('fi',9,500),('fl',10,500),('ffi',11,770)]:
            self.assertEqual(self.shape(text),[(gid,0,width)])

    def test_off_retains_separate_letters(self):
        self.assertEqual([g for g,c,w in self.shape('ffi',liga=0)],[2,2,3])

    def test_discretionary_requires_explicit_flag(self):
        self.assertEqual(len(self.shape('st')),2)
        self.assertEqual(self.shape('st',dlig=1),[(12,0,550)])

    def test_kerning_survives_ligature_choice(self):
        for liga in [0,1]:
            self.assertEqual(sum(w for g,c,w in self.shape('AV',liga=liga)),1120)
            self.assertEqual(sum(w for g,c,w in self.shape('AV',liga=liga,kern=0)),1200)

    def test_broken_run_reproduction_and_rejoined_control(self):
        # Demonstrates the exact shaping-boundary cause with real GSUB.
        split=self.shape('f')+self.shape('i')
        joined=self.shape('fi')
        self.assertEqual(len(split),2);self.assertEqual(len(joined),1)
        self.assertGreater(sum(w for g,c,w in split),sum(w for g,c,w in joined))


if __name__=='__main__':
    unittest.main(verbosity=2)
