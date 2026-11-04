// Draws the Couchpad app icon: a white controller and steering circle on a blue rounded square.
// Every shape is drawn here; SF Symbols may not be used in app icons.
// Usage: swift make-icon.swift out.png   (writes a 1024×1024 PNG)

import AppKit

let size: CGFloat = 1024
let output = CommandLine.arguments.dropFirst().first ?? "icon.png"

func rotated(_ path: NSBezierPath, by degrees: CGFloat, around centre: NSPoint) -> NSBezierPath {
    var transform = AffineTransform(translationByX: centre.x, byY: centre.y)
    transform.rotate(byDegrees: degrees)
    transform.translate(x: -centre.x, y: -centre.y)
    let copy = path.copy() as! NSBezierPath
    copy.transform(using: transform)
    return copy
}

/// The controller in white, with the D-pad and buttons cut out so the background shows through.
func controller() -> NSImage {
    NSImage(size: NSSize(width: size, height: size), flipped: false) { _ in
        NSColor.white.setFill()
        NSBezierPath(roundedRect: NSRect(x: 232, y: 440, width: 560, height: 230), xRadius: 115, yRadius: 115).fill()
        for (x, angle) in [(CGFloat(330), CGFloat(-24)), (CGFloat(694), CGFloat(24))] {
            let grip = NSBezierPath(roundedRect: NSRect(x: x - 85, y: 330, width: 170, height: 300), xRadius: 85, yRadius: 85)
            rotated(grip, by: angle, around: NSPoint(x: x, y: 560)).fill()
        }

        NSGraphicsContext.current?.compositingOperation = .clear
        NSBezierPath(roundedRect: NSRect(x: 290, y: 535, width: 130, height: 40), xRadius: 14, yRadius: 14).fill()
        NSBezierPath(roundedRect: NSRect(x: 335, y: 490, width: 40, height: 130), xRadius: 14, yRadius: 14).fill()
        for (x, y) in [(CGFloat(680), CGFloat(590)), (CGFloat(620), CGFloat(525))] {
            NSBezierPath(ovalIn: NSRect(x: x - 34, y: y - 34, width: 68, height: 68)).fill()
        }
        return true
    }
}

let image = NSImage(size: NSSize(width: size, height: size), flipped: false) { _ in
    // Apple's icon grid: an 824 pt rounded square centred on the 1024 pt canvas.
    let tile = NSRect(x: 100, y: 100, width: 824, height: 824)
    let shape = NSBezierPath(roundedRect: tile, xRadius: 185, yRadius: 185)

    NSGraphicsContext.current?.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.35)
    shadow.shadowBlurRadius = 24
    shadow.shadowOffset = NSSize(width: 0, height: -10)
    shadow.set()
    NSColor.black.setFill()
    shape.fill()
    NSGraphicsContext.current?.restoreGraphicsState()

    NSGradient(
        starting: NSColor(calibratedRed: 0.20, green: 0.55, blue: 1.00, alpha: 1),
        ending: NSColor(calibratedRed: 0.27, green: 0.18, blue: 0.75, alpha: 1)
    )?.draw(in: shape, angle: -90)

    controller().draw(in: NSRect(x: 0, y: 0, width: size, height: size))

    // The steering circle with its centre dot, as shown while the stick moves the pointer.
    let centre = NSPoint(x: 512, y: 235)
    let ring = NSBezierPath(ovalIn: NSRect(x: centre.x - 70, y: centre.y - 70, width: 140, height: 140))
    ring.lineWidth = 22
    NSColor.white.setStroke()
    ring.stroke()
    NSColor.white.setFill()
    NSBezierPath(ovalIn: NSRect(x: centre.x - 14, y: centre.y - 14, width: 28, height: 28)).fill()
    return true
}

guard let tiff = image.tiffRepresentation,
      let bitmap = NSBitmapImageRep(data: tiff),
      let png = bitmap.representation(using: .png, properties: [:])
else {
    fatalError("could not render the icon")
}
try png.write(to: URL(fileURLWithPath: output))
