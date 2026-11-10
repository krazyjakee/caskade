import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("L").crop((264, 90, 1376, 830))
h = im.histogram(); n = sum(h)
print(f"{sys.argv[1].split('/')[-1]}: mean={sum(i*c for i,c in enumerate(h))/n:.1f} bright%={100*sum(h[60:])/n:.2f}")
