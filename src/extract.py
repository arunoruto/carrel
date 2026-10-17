# carrel's default extractor: $PDF -> fulltext.md + figures/page-NNNN-MM.png in the current directory.
# Replace it by setting CARREL_EXTRACTOR to any shell command that does the same.
import pathlib
import sys

import pymupdf
import pymupdf4llm

# Opened from bytes, so figures are named after their page instead of the PDF's file name.
doc = pymupdf.open(stream=pathlib.Path(sys.argv[1]).read_bytes(), filetype="pdf")
md = pymupdf4llm.to_markdown(doc, filename="page", write_images=True, image_path="figures", image_format="png")
pathlib.Path("fulltext.md").write_text(md, encoding="utf-8")
