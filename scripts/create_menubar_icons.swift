// Run from the repository root:
// swift scripts/create_menubar_icons.swift [preview.png]
// The PDFs are transparent, resolution-independent AppKit template images.
import AppKit
import CoreGraphics

let size: CGFloat = 18
let output = URL(fileURLWithPath: "resources/images", isDirectory: true)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)

enum IconState: String, CaseIterable {
    case idle, active, failed
}

func drawIcon(_ context: CGContext, state: IconState) {
    let outline = CGPath(roundedRect: CGRect(x: 1, y: 1, width: 14.5, height: 14.5),
                         cornerWidth: 3, cornerHeight: 3, transform: nil)
    context.saveGState()
    if state != .idle {
        // The cutout follows the badge contour, leaving a narrow transparent halo.
        context.addRect(CGRect(x: 0, y: 0, width: size, height: size))
        if state == .active {
            context.addEllipse(in: CGRect(x: 9.3, y: 9.3, width: 9.4, height: 9.4))
        } else {
            let halo = CGMutablePath()
            halo.move(to: CGPoint(x: 12.65, y: 20.2))
            halo.addLine(to: CGPoint(x: 20.4, y: 6.6))
            halo.addLine(to: CGPoint(x: 5.2, y: 6.6))
            halo.closeSubpath()
            context.addPath(halo)
        }
        context.clip(using: .evenOdd)
    }
    context.setStrokeColor(CGColor(gray: 0, alpha: 1))
    context.setLineWidth(1.25)
    context.addPath(outline)
    context.strokePath()
    context.addPath(outline)
    context.clip()
    context.setLineWidth(1.15)
    for offset in stride(from: -15, through: 15, by: 5) {
        context.move(to: CGPoint(x: CGFloat(offset), y: 0))
        context.addLine(to: CGPoint(x: CGFloat(offset) + size, y: size))
    }
    context.strokePath()
    context.restoreGState()

    if state == .active {
        context.setFillColor(CGColor(gray: 0, alpha: 1))
        context.fillEllipse(in: CGRect(x: 10.5, y: 10.5, width: 7, height: 7))
    } else if state == .failed {
        let badge = CGMutablePath()
        badge.move(to: CGPoint(x: 12.9, y: 17.6))
        badge.addLine(to: CGPoint(x: 18, y: 8.5))
        badge.addQuadCurve(to: CGPoint(x: 17.5, y: 7.8), control: CGPoint(x: 18.4, y: 7.8))
        badge.addLine(to: CGPoint(x: 8.3, y: 7.8))
        badge.addQuadCurve(to: CGPoint(x: 7.8, y: 8.5), control: CGPoint(x: 7.4, y: 7.8))
        badge.addLine(to: CGPoint(x: 12.4, y: 17.6))
        badge.closeSubpath()
        badge.addRoundedRect(in: CGRect(x: 12.25, y: 11.1, width: 1.1, height: 3.5),
                             cornerWidth: 0.5, cornerHeight: 0.5)
        badge.addEllipse(in: CGRect(x: 12.22, y: 9.1, width: 1.16, height: 1.16))
        context.setFillColor(CGColor(gray: 0, alpha: 1))
        context.addPath(badge)
        context.fillPath(using: .evenOdd)
    }
}

for state in IconState.allCases {
    let url = output.appendingPathComponent("menubar-\(state.rawValue).pdf")
    var bounds = CGRect(x: 0, y: 0, width: size, height: size)
    guard let pdf = CGContext(url as CFURL, mediaBox: &bounds, nil) else {
        fatalError("Cannot create \(url.path)")
    }
    pdf.beginPDFPage(nil)
    drawIcon(pdf, state: state)
    pdf.endPDFPage()
    pdf.closePDF()
    print("Created \(url.path)")
}

// Optional preview renders the actual PDF assets through NSImage, just like the app.
if let path = CommandLine.arguments.dropFirst().first {
    let width = 1080, height = 280
    let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    let graphics = NSGraphicsContext(bitmapImageRep: bitmap)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = graphics
    for dark in [false, true] {
        let x: CGFloat = dark ? 540 : 0
        (dark ? NSColor(calibratedWhite: 0.12, alpha: 1) : NSColor.white).setFill()
        NSRect(x: x, y: 0, width: 540, height: 280).fill()
        let color = dark ? NSColor.white : NSColor.black
        for (index, state) in IconState.allCases.enumerated() {
            let center = x + CGFloat(index) * 170 + 95
            let url = output.appendingPathComponent("menubar-\(state.rawValue).pdf")
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
            let label = state.rawValue.capitalized
            (label as NSString).draw(at: NSPoint(x: center - 20, y: 30), withAttributes: [
                .font: NSFont.systemFont(ofSize: 13), .foregroundColor: color,
            ])
        }
    }
    NSGraphicsContext.restoreGraphicsState()
    try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
    print("Created preview \(path)")
}
