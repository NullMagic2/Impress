"""Runtime coverage must inspect messages, not source comments or authored data."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('runtime_audit',ROOT/'tools/audit_runtime_translations.py')
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)

class RuntimeTranslationTests(unittest.TestCase):
    def test_comments_documents_fonts_and_cli_are_not_messages(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'crates/gui/src';source.mkdir(parents=True)
            (source/'main.rs').write_text('''
// Err("A comment is not a message")
/* Err("Nor is a nested /* block */ comment") */
let document_text = "A personal letter";
let font = "Noto Sans";
let message = Err("A missing picture");
let dynamic = Err(format!("Could not read {path}"));
let caption = Button::builder().with_label("Recent files");
let wrapped = i18n::text(&("Wrapped actual error"));
let string = "https://example.org/*not a comment*/";
let other = Err("Another actual error");
#[cfg(test)] mod tests { let fixture = Err("Test only error"); }
''')
            cli=source/'bin';cli.mkdir();(cli/'tool.rs').write_text('Err("CLI only error")')
            self.assertEqual(set(audit.runtime_messages(root)),{
                'A missing picture','Could not read {path}','Recent files','Another actual error','Wrapped actual error'})

    def test_missing_catalog_entries_are_reported(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'crates/gui/src';source.mkdir(parents=True)
            (source/'main.rs').write_text('Err("A missing picture"); Err("Another actual error");')
            self.assertEqual(audit.missing_messages(root,{'A missing picture':['x']*4}),
                             {'Another actual error':['crates/gui/src/main.rs']})

    def test_painted_ribbon_captions_include_disabled_commands(self):
        import json
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'crates').mkdir()
            manifest=root/'crates/gui/ui/ribbon-manifest.json';manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({'buttons':[
                {'label':'Link','tip':'Insert a link','disabled':True},
                {'label':'PDF','tip':'','disabled':True}]}))
            self.assertEqual(set(audit.runtime_messages(root)),{'Link','Insert a link'})

    def test_all_custom_dialog_button_arguments_must_be_translated(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);source=root/'crates/gui/src';source.mkdir(parents=True)
            (source/'main.rs').write_text('''// dialog.set_ok_cancel_labels("Comment", "Ignored");
dialog.set_ok_cancel_labels(&i18n::text("Save"), "Keep editing");
dialog.set_yes_no_labels("Delete", &i18n::text("Cancel"));
dialog.set_ok_cancel_labels(&i18n::text("Save"), &i18n::text("Cancel"));
label.set_label(&format!("{} · {}", name, text("Matches")));
label.set_label("%");
''')
            actual=audit.unlocalized_labels(root)
            self.assertEqual([(v['argument'],v['text']) for v in actual],[(1,'Keep editing'),(0,'Delete')])

    def test_native_catalogs_match_all_four_interface_translations(self):
        import gettext,json
        locales=ROOT/'crates/gui/assets/locales'
        catalog=json.loads((locales/'translations.json').read_text())
        for index,language in enumerate(['el','es','pt_BR','pt_PT']):
            with (locales/(language+'.mo')).open('rb') as stream:native=gettext.GNUTranslations(stream)
            for key,row in catalog.items():self.assertEqual(native.gettext(key),row[index],(language,key))

class CatalogTerminologyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import json
        cls.catalog=json.loads((ROOT/'crates/gui/assets/locales/translations.json').read_text())

    def test_free_positioning_never_means_free_of_charge(self):
        import re
        for key,values in self.catalog.items():
            if 'free' in key.lower() and any(term in key.lower() for term in ['table','text','position']):
                for value in values:
                    self.assertIsNone(re.search(r'δωρεάν|grátis|gratis|gratuit',value,re.I),(key,value))

    def test_caret_is_an_editing_cursor_and_tabs_are_typographical(self):
        import re
        for key,values in self.catalog.items():
            if 'caret' in key.lower():
                for value in values:
                    self.assertIsNone(re.search(r'φροντί|cuidado|custodia|alcoó',value,re.I),(key,value))
            if 'tab stop' in key.lower() or 'tab position' in key.lower():
                for value,term in zip(values,['στηλοθε','tabula','tabula','tabula']):
                    import unicodedata
                    plain=''.join(c for c in unicodedata.normalize('NFD',value.lower()) if not unicodedata.combining(c))
                    self.assertIn(term,plain,(key,value))

    def test_math_commands_and_font_family_identifiers_survive_translation(self):
        for name in ['cos','exp','ln','log','sin','tan']:
            for value in self.catalog['type '+name]:self.assertIn(name,value)
        for key in ['Neo Aegean','Neo Aegean Compact Light','[Content_Types].xml','font-style:italic;','mailto:']:
            self.assertEqual(self.catalog[key],[key]*4)

    def test_numeric_limits_are_not_dropped_or_corrupted(self):
        import re
        from collections import Counter
        def numbers(value):
            return Counter(re.findall(r'\d+',re.sub(r'\{[^{}]*\}', '',value)))
        for key,values in self.catalog.items():
            # Singular translations may say "the whole page" without the digit.
            if key=='All 1 page':continue
            for value in values:self.assertEqual(numbers(key),numbers(value),(key,value))

    def test_literal_input_examples_remain_usable(self):
        for key,values in self.catalog.items():
            for token in ['#RRGGBB','sqrt(a)','x^2','1/2','libenchant-2-2','hunspell-en-us','hunspell-pt-br','comments.xml','numbering.xml','w:numbering']:
                if token in key:
                    for value in values:self.assertIn(token,value,(key,value))
