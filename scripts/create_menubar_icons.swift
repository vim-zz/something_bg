// Run from the repository root:
// swift scripts/create_menubar_icons.swift [preview.png]
// The PDFs are transparent, resolution-independent AppKit template images.
import AppKit
import CoreGraphics

let size: CGFloat = 18
let output = URL(fileURLWithPath: "resources/images", isDirectory: true)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)

func drawIcon(_ context: CGContext, active: Bool) {
    let outline = CGPath(roundedRect: CGRect(x: 1, y: 1, width: 16, height: 16),
                         cornerWidth: 3, cornerHeight: 3, transform: nil)
    context.setStrokeColor(CGColor(gray: 0, alpha: 1))
    context.setLineWidth(1.25)
    context.addPath(outline)
    context.strokePath()

    context.saveGState()
    context.addPath(outline)
    context.clip()
    if active {
        // Clear the hatch field around the solid circle, leaving a transparent halo.
        // Both states share the same boundary and stripe positions.
        context.addRect(CGRect(x: 0, y: 0, width: size, height: size))
        context.addEllipse(in: CGRect(x: 4.5, y: 4.5, width: 9, height: 9))
        context.clip(using: .evenOdd)
    }
    context.setLineWidth(1.15)
    for offset in stride(from: -15, through: 15, by: 5) {
        context.move(to: CGPoint(x: CGFloat(offset), y: 0))
        context.addLine(to: CGPoint(x: CGFloat(offset) + size, y: size))
    }
    context.strokePath()
    context.restoreGState()

    if active {
        context.setFillColor(CGColor(gray: 0, alpha: 1))
        context.fillEllipse(in: CGRect(x: 5.75, y: 5.75, width: 6.5, height: 6.5))
    }
}

for active in [false, true] {
    let url = output.appendingPathComponent(active ? "menubar-active.pdf" : "menubar-idle.pdf")
    var bounds = CGRect(x: 0, y: 0, width: size, height: size)
    guard let pdf = CGContext(url as CFURL, mediaBox: &bounds, nil) else {
        fatalError("Cannot create \(url.path)")
    }
    pdf.beginPDFPage(nil)
    drawIcon(pdf, active: active)
    pdf.endPDFPage()
    pdf.closePDF()
    print("Created \(url.path)")
}

// Optional preview renders the actual PDF assets through NSImage, just like the app.
if let path = CommandLine.arguments.dropFirst().first {
    let width = 720, height = 280
    let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    let graphics = NSGraphicsContext(bitmapImageRep: bitmap)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = graphics
    for dark in [false, true] {
        let x: CGFloat = dark ? 360 : 0
        (dark ? NSColor(calibratedWhite: 0.12, alpha: 1) : NSColor.white).setFill()
        NSRect(x: x, y: 0, width: 360, height: 280).fill()
        let color = dark ? NSColor.white : NSColor.black
        for (index, active) in [false, true].enumerated() {
            let center = x + CGFloat(index) * 170 + 95
            let url = output.appendingPathComponent(active ? "menubar-active.pdf" : "menubar-idle.pdf")
            let source = NSImage(contentsOf: url)!
            // Tint only the alpha coverage, preserving the transparent halo.
            let image = NSImage(size: NSSize(width: size, height: size), flipped: false) { rect in
                source.draw(in: rect)
                color.setFill()
                rect.fill(using: .sourceIn)
                return true
            }
            image.draw(in: NSRect(x: center - 54, y: 115, width: 108, height: 108))
            image.draw(in: NSRect(x: center - 9, y: 62, width: 18, height: 18))
            let label = active ? "Active" : "Idle"
            (label as NSString).draw(at: NSPoint(x: center - 20, y: 30), withAttributes: [
                .font: NSFont.systemFont(ofSize: 13), .foregroundColor: color,
            ])
        }
    }
    NSGraphicsContext.restoreGraphicsState()
    try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
    print("Created preview \(path)")
}
