#!/bin/bash
# Script to create circle app icon for Something in the Background

# Create iconset directory
mkdir -p circle.iconset

# Generate circle images at different sizes using Python
source .venv/bin/activate
python3 << 'EOF'
from PIL import Image, ImageDraw

def create_circle_icon(size, filename):
    # Create a transparent image
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Draw a ring (circle outline) with some padding
    padding = int(size * 0.1)
    circle_bbox = [padding, padding, size - padding, size - padding]

    # Use a dark gray/black color for the ring
    ring_color = (60, 60, 60, 255)  # Dark gray, fully opaque
    # Calculate stroke width proportional to size
    stroke_width = max(1, int(size * 0.08))
    draw.ellipse(circle_bbox, outline=ring_color, width=stroke_width)

    # Save the image
    img.save(filename, 'PNG')
    print(f"Created {filename}")

# Standard macOS icon sizes
sizes = [16, 32, 64, 128, 256, 512, 1024]

for size in sizes:
    create_circle_icon(size, f"circle.iconset/icon_{size}x{size}.png")
    if size <= 512:
        # Create @2x version
        create_circle_icon(size * 2, f"circle.iconset/icon_{size}x{size}@2x.png")

print("All icon sizes created")
EOF

# Convert iconset to icns using macOS iconutil
iconutil -c icns circle.iconset -o resources/circle.icns

# Clean up
rm -rf circle.iconset

echo "Created resources/circle.icns"
