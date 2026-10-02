import AppKit

// Run on macOS: swift scripts/generate-app-icon.swift
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let assets = root.appendingPathComponent("assets")
let temporary = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
let iconset = temporary.appendingPathComponent("BreakReminder.iconset")
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: temporary) }

func png(size: Int) throws -> Data {
    let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    )!
    NSGraphicsContext.saveGraphicsState()
    defer { NSGraphicsContext.restoreGraphicsState() }
    let context = NSGraphicsContext(bitmapImageRep: bitmap)!
    NSGraphicsContext.current = context
    context.cgContext.clear(CGRect(x: 0, y: 0, width: size, height: size))
    let scale = CGFloat(size) / 1024
    context.cgContext.scaleBy(x: scale, y: scale)

    let tile = NSBezierPath(roundedRect: NSRect(x: 80, y: 80, width: 864, height: 864),
                            xRadius: 184, yRadius: 184)
    NSGraphicsContext.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.18)
    shadow.shadowBlurRadius = 20
    shadow.shadowOffset = NSSize(width: 0, height: -10)
    shadow.set()
    NSColor(srgbRed: 24 / 255, green: 143 / 255, blue: 118 / 255, alpha: 1).setFill()
    tile.fill()
    NSGraphicsContext.restoreGraphicsState()

    NSColor.white.setStroke()
    let circle = NSBezierPath(ovalIn: NSRect(x: 272, y: 272, width: 480, height: 480))
    circle.lineWidth = 52
    circle.stroke()
    let hands = NSBezierPath()
    hands.move(to: NSPoint(x: 512, y: 672))
    hands.line(to: NSPoint(x: 512, y: 512))
    hands.line(to: NSPoint(x: 640, y: 512))
    hands.lineWidth = 52
    hands.lineCapStyle = .round
    hands.lineJoinStyle = .round
    hands.stroke()
    return bitmap.representation(using: .png, properties: [:])!
}

for size in [16, 32, 128, 256, 512] {
    try png(size: size).write(to: iconset.appendingPathComponent("icon_\(size)x\(size).png"))
    try png(size: size * 2).write(to: iconset.appendingPathComponent("icon_\(size)x\(size)@2x.png"))
}
try png(size: 1024).write(to: assets.appendingPathComponent("app-icon.png"))
let export = Process()
export.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
export.arguments = ["-c", "icns", "-o", assets.appendingPathComponent("app-icon.icns").path, iconset.path]
try export.run()
export.waitUntilExit()
guard export.terminationStatus == 0 else {
    try FileManager.default.removeItem(at: temporary)
    fputs("iconutil failed to export the app icon\n", stderr)
    exit(1)
}
print("Wrote assets/app-icon.png and assets/app-icon.icns")
