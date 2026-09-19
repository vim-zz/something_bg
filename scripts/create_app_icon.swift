// Render the approved menu bar mark onto a macOS app-icon tile.
// Invoked by scripts/create_icons.sh from any working directory.
import AppKit

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let output = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let mark = NSImage(contentsOf: root.appendingPathComponent("resources/images/menubar-active.pdf"))!

func render(_ size: Int, to url: URL) throws {
    let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    let graphics = NSGraphicsContext(bitmapImageRep: bitmap)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = graphics
    let context = graphics.cgContext
    context.scaleBy(x: CGFloat(size) / 1024, y: CGFloat(size) / 1024)

    let tile = NSBezierPath(roundedRect: NSRect(x: 100, y: 100, width: 824, height: 824),
                            xRadius: 185, yRadius: 185)
    context.saveGState()
    context.setShadow(offset: CGSize(width: 0, height: -9), blur: 20,
                      color: NSColor.black.withAlphaComponent(0.18).cgColor)
    NSColor(calibratedWhite: 0.97, alpha: 1).setFill()
    tile.fill()
    context.restoreGState()
    NSGradient(starting: NSColor(calibratedWhite: 1, alpha: 1),
               ending: NSColor(calibratedWhite: 0.91, alpha: 1))!.draw(in: tile, angle: -90)

    // Reuse the actual PDF so the circle, halo, and stripes match the menu bar.
    let tinted = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { rect in
        mark.draw(in: rect)
        NSColor(calibratedWhite: 0.12, alpha: 1).setFill()
        rect.fill(using: .sourceIn)
        return true
    }
    // Center the striped rectangle, not the PDF's combined rectangle-and-badge
    // bounds. Its geometry in create_menubar_icons.swift is (1, 1, 14.5, 14.5).
    // The badge extends above and to the right without shifting the background.
    let background = NSRect(x: 1, y: 1, width: 14.5, height: 14.5)
    let markScale: CGFloat = 612 / 18
    tinted.draw(in: NSRect(x: 512 - background.midX * markScale,
                          y: 512 - background.midY * markScale,
                          width: 612, height: 612))
    NSGraphicsContext.restoreGraphicsState()
    try bitmap.representation(using: .png, properties: [:])!.write(to: url)
}

for size in [16, 32, 128, 256, 512] {
    try render(size, to: output.appendingPathComponent("icon_\(size)x\(size).png"))
    try render(size * 2, to: output.appendingPathComponent("icon_\(size)x\(size)@2x.png"))
}
