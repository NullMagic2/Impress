# OCR models

`text-detection.rten` and `text-recognition.rten` are the pre-trained models
of the [ocrs](https://github.com/robertknight/ocrs) OCR engine by Robert
Knight, from <https://huggingface.co/robertknight/ocrs> (downloaded from
`ocrs-models.s3-accelerate.amazonaws.com`). They are licensed under the
Creative Commons Attribution-ShareAlike 4.0 licence (CC BY-SA 4.0), and were
trained on the HierText dataset (CC BY-SA 4.0).

Impress embeds them in `crates/format-pdf` (`src/ocr.rs`) and uses them only
when Tesseract is not installed. They recognise unaccented Latin letters, so
their text is kept only for documents that read as English.

SHA-256:

- `text-detection.rten`: f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca
- `text-recognition.rten`: e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e
