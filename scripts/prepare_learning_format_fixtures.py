#!/usr/bin/env python3
"""Generate independent P6 fixture files; optional test-only python-docx/reportlab, no product dependency."""
from pathlib import Path
import hashlib,json,zipfile
from docx import Document
from reportlab.pdfgen import canvas
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont
from reportlab.lib.pdfencrypt import StandardEncryption
from PIL import Image,ImageDraw
ROOT=Path(__file__).resolve().parents[1]/'research/style-aggregation/runs/learning-formats-local-01'
def main():
    out=ROOT/'fixtures';out.mkdir(parents=True,exist_ok=True)
    d=Document();d.add_heading('雨夜读书',0);p=d.add_paragraph();p.add_run('我合上书，');p.add_run('窗外的雨还没有停。')
    p=d.add_paragraph('甲 & 乙之间留一个空格。');p.add_run('隐藏文字不进入正文').font.hidden=True
    table=d.add_table(rows=1,cols=2);table.cell(0,0).text='左格';table.cell(0,1).text='右格'
    d.save(out/'中文文章.docx')
    expected='雨夜读书\n\n我合上书，窗外的雨还没有停。\n\n甲 & 乙之间留一个空格。\n\n左格\n\n右格'
    (out/'重复.md').write_text(expected+'\n')
    d=Document();d.add_heading('纸上候车',1);d.add_paragraph('车票夹在书里，这是一篇完整短文。');d.add_heading('项目备忘',1);d.add_paragraph('上线前检查日志，这是另一篇独立短文。');d.save(out/'两篇文章.docx')
    im=Image.new('RGB',(120,40),'white');draw=ImageDraw.Draw(im);draw.text((5,10),'SCANNED ARTICLE',fill='black');im.save(out/'scan.png')
    d=Document();d.add_paragraph('只有图片的文章无法保证读全。');d.add_picture(str(out/'scan.png'));d.save(out/'含图文章.docx')
    d=Document();d.add_paragraph('长文章内容。'*20000);d.save(out/'长文章.docx')
    (out/'损坏.docx').write_bytes(b'not a Word package')
    with zipfile.ZipFile(out/'解压超限.docx','w',zipfile.ZIP_DEFLATED) as z:z.writestr('word/document.xml',b'x'*(8*1024*1024+1))
    pdfmetrics.registerFont(UnicodeCIDFont('STSong-Light'))
    def text(c,s):c.setFont('STSong-Light',12);c.drawString(50,750,s)
    c=canvas.Canvas(str(out/'中文文字.pdf'));text(c,'月色与书页。');c.showPage();text(c,'同一篇文章的第二页，读完后我关了灯。');c.save()
    c=canvas.Canvas(str(out/'空白加正文.pdf'));c.showPage();text(c,'空白页后面仍是完整正文。');c.save()
    c=canvas.Canvas(str(out/'扫描.pdf'));c.drawImage(str(out/'scan.png'),50,500);c.save()
    c=canvas.Canvas(str(out/'混合扫描.pdf'));text(c,'这是第一面的文字。');c.showPage();c.drawImage(str(out/'scan.png'),50,500);c.save()
    c=canvas.Canvas(str(out/'加密.pdf'),encrypt=StandardEncryption('fixture-password'));text(c,'加密文件不自动猜密码。');c.save()
    (out/'损坏.pdf').write_bytes(b'%PDF-1.5\nbroken file')
    (out/'原始.doc').write_bytes(b'unsupported binary Word')
    manifest={'generators':{'python-docx':__import__('docx').__version__,'reportlab':__import__('reportlab').Version},'expected_docx':expected,'expected_pdf_pages':['月色与书页。','同一篇文章的第二页，读完后我关了灯。'],'files':{p.name:{'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in out.iterdir()}}
    (ROOT/'fixture-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'files':len(manifest['files']),'directory':str(out)},ensure_ascii=False))
if __name__=='__main__':main()
