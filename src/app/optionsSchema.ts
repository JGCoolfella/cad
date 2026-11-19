/**
 * Tools > Options, described as data: the System Options tree and the Document
 * Properties tree, with every control on each page. Page structure, labels and
 * defaults were taken from SOLIDWORKS 2026 Options.
 *
 * Keys: a system key that matches a path in SystemOptions (e.g. "view.reverseWheelZoom")
 * reads and writes that live setting. Any other key is stored in `options.extra`
 * (system) or `doc.props` (document). `live: true` marks controls that already change
 * OpenCAD's behaviour; the others are stored but have no effect yet, and the dialog
 * says so.
 */

export type Choice = [value: string, label: string];

export type Ctl =
  | { t: "check"; key: string; label: string; def: boolean; indent?: number; live?: boolean; disabled?: boolean }
  | { t: "radio"; key: string; label?: string; options: Choice[]; def: string; live?: boolean }
  | { t: "select"; key: string; label: string; options: Choice[]; def: string; live?: boolean }
  | { t: "number"; key: string; label: string; def: number; unit?: string; min?: number; max?: number; step?: number; live?: boolean }
  | { t: "slider"; key: string; label: string; def: number; min: number; max: number; step: number; left?: string; right?: string; live?: boolean }
  | { t: "text"; key: string; label: string; def: string }
  | { t: "color"; key: string; label: string; def: string; live?: boolean }
  | { t: "button"; label: string; action?: string }
  | { t: "group"; label: string; items: Ctl[] }
  | { t: "note"; text: string }
  | { t: "colorScheme" }
  | { t: "unitsTable" }
  | { t: "featureColors" };

export interface Page {
  id: string;
  label: string;
  items: Ctl[];
  children?: Page[];
}

const chk = (key: string, label: string, def: boolean, extra: Partial<Extract<Ctl, { t: "check" }>> = {}): Ctl => ({ t: "check", key, label, def, ...extra });
const live = (key: string, label: string, def: boolean, extra: Partial<Extract<Ctl, { t: "check" }>> = {}): Ctl => chk(key, label, def, { live: true, ...extra });
const group = (label: string, ...items: Ctl[]): Ctl => ({ t: "group", label, items });
const slider = (key: string, label: string, def: number, left = "Slow", right = "Fast", live = false): Ctl => ({ t: "slider", key, label, def, min: 0, max: 10, step: 1, left, right, live });

// ------------------------------------------------------------------ System Options

export const SYSTEM_PAGES: Page[] = [
  {
    id: "general",
    label: "General",
    items: [
      group(
        "Recent documents",
        { t: "number", key: "general.maxRecent", label: "Maximum recent documents displayed:", def: 50, min: 0, max: 100 },
        chk("general.includeOther", "Include documents opened from other documents", false),
        { t: "select", key: "general.openLast", label: "Open last used document(s) at startup:", options: [["never", "Never"], ["always", "Always"]], def: "never" },
      ),
      live("sketch.inputDimensionValue", "Input dimension value", true),
      chk("general.singleCommandPerPick", "Single command per pick", false),
      chk("general.thumbnails", "Show thumbnail graphics in Windows Explorer", true),
      { t: "button", label: "Restore File Associations" },
      chk("general.systemSeparator", "Use system separator for dimensions", true),
      chk("general.englishMenus", "Use English language menus", false, { disabled: true }),
      chk("general.englishNames", "Use English language feature and file names", false, { disabled: true }),
      live("general.confirmationCorner", "Enable Confirmation Corner", true),
      chk("general.autoShowPM", "Auto-show PropertyManager", true),
      chk("general.autoSizePM", "Auto-size PropertyManager when panels are split", true),
      chk("general.editMacro", "Automatically edit macro after recording", false),
      chk("general.stopVsta", "Stop VSTA debugger on macro exit", true),
      chk("general.featureXpert", "Enable FeatureXpert", true),
      chk("general.freezeBar", "Enable Freeze bar", false),
      { t: "select", key: "general.componentDescription", label: "Custom property used as component description:", options: [["description", "Description"]], def: "description" },
      chk("general.checkSolutions", "Check for solutions when the application crashes", true),
      chk("general.sounds", "Enable sounds for events", true),
      { t: "button", label: "Configure Sounds..." },
    ],
  },
  {
    id: "drawings",
    label: "Drawings",
    items: [
      { t: "radio", key: "drawings.zoomOnOpen", label: "Zoom level to open drawings", options: [["lastSaved", "Zoom to last saved"], ["fit", "Zoom to fit"], ["sheet", "Zoom to sheet"]], def: "lastSaved" },
      chk("drawings.dupDims", "Eliminate duplicate model dimensions on insert", true),
      chk("drawings.dupNotes", "Eliminate duplicate model notes on insert", true),
      chk("drawings.markDims", "Mark all part/assembly dimension for import into drawings by default", true),
      chk("drawings.autoScale", "Automatically scale new drawing views", true),
      chk("drawings.revSymbol", "Enable symbol when adding new revision", true),
      chk("drawings.selectHidden", "Select hidden entities", false),
      chk("drawings.noInference", "Disable note/dimension inference", false),
      chk("drawings.noMerge", "Disable note merging when dragging", true),
      chk("drawings.watermark", "Print out-of-sync water mark", true),
      chk("drawings.refNames", "Show reference geometry names in drawings", false),
      chk("drawings.hideComponents", "Automatically hide components on view creation", false),
      chk("drawings.arcCenters", "Display sketch arc centerpoints", false),
      chk("drawings.entityPoints", "Display sketch entity points", false),
      chk("drawings.hatchBehind", "Display sketch hatch behind geometry", false),
      chk("drawings.picturesBehind", "Display sketch pictures on sheet behind geometry", false),
      chk("drawings.breaklines", "Print breaklines in break view", true),
      chk("drawings.alignBreaks", "Align breaks with parent view for projected view", true),
      chk("drawings.viewPalette", "Automatically populate View Palette with views", true),
      chk("drawings.sheetFormatDialog", "Show sheet format dialog when adding new sheet", false),
    ],
    children: [
      {
        id: "drawings.displayStyle",
        label: "Display Style",
        items: [
          { t: "radio", key: "drawings.style", label: "Display style", options: [["wireframe", "Wireframe"], ["hlv", "Hidden lines visible"], ["hlr", "Hidden lines removed"], ["shadedEdges", "Shaded with edges"], ["shaded", "Shaded"]], def: "hlr" },
          { t: "radio", key: "drawings.tangentEdges", label: "Tangent edges", options: [["visible", "Visible"], ["fonts", "Use font"], ["removed", "Removed"]], def: "visible" },
          { t: "radio", key: "drawings.quality", label: "Edge quality for new views", options: [["high", "High quality"], ["draft", "Draft quality"]], def: "high" },
        ],
      },
      {
        id: "drawings.hatch",
        label: "Area Hatch/Fill",
        items: [
          { t: "radio", key: "drawings.hatchType", options: [["none", "None"], ["solid", "Solid"], ["hatch", "Hatch"]], def: "hatch" },
          { t: "select", key: "drawings.hatchPattern", label: "Pattern:", options: [["ansi31", "ANSI31 (Iron BrickStone)"]], def: "ansi31" },
          { t: "number", key: "drawings.hatchScale", label: "Scale:", def: 1 },
          { t: "number", key: "drawings.hatchAngle", label: "Angle:", def: 0, unit: "deg" },
        ],
      },
      {
        id: "drawings.performance",
        label: "Performance",
        items: [
          chk("drawings.perf.dragShow", "Show contents while dragging drawing view", true),
          chk("drawings.perf.autoUpdate", "Allow auto-update when opening drawings", true),
          chk("drawings.perf.saveTess", "Save tessellated data for drawings with shaded and draft quality views", true),
        ],
      },
    ],
  },
  { id: "colors", label: "Colors", items: [{ t: "colorScheme" }] },
  {
    id: "sketch",
    label: "Sketch",
    items: [
      live("sketch.autoRotateNormal", "Auto-rotate view normal to sketch plane on sketch creation and sketch edit", true),
      chk("sketch.useFullyDefined", "Use fully defined sketches", false),
      live("sketch.arcCenters", "Display arc centerpoints in part/assembly sketches", true),
      live("sketch.entityPoints", "Display entity points in part/assembly sketches", true),
      chk("sketch.promptClose", "Prompt to close sketch", true),
      chk("sketch.onNewPart", "Create sketch on new part", false),
      chk("sketch.overrideDims", "Override dimensions on drag/move", false),
      chk("sketch.planeWhenShaded", "Display plane when shaded", false),
      chk("sketch.virtualSharps3d", "Line length measured between virtual sharps in 3d", true),
      chk("sketch.splineHandles", "Enable spline tangency and curvature handles", true),
      chk("sketch.splinePolygon", "Show spline control polygon by default", false),
      chk("sketch.ghost", "Ghost image on drag", true),
      chk("sketch.combBounding", "Show curvature comb bounding curve", false),
      live("sketch.scaleOnFirst", "Scale sketch on first dimension creation", true),
      chk("sketch.onScreenNumeric", "Enable on screen numeric input on entity creation", false),
      chk("sketch.onlyWhenEntered", "Create dimension only when value is entered", false, { indent: 1, disabled: true }),
      chk("sketch.previewDim", "Preview sketch dimension when selected", false),
      group("Over defining dimensions", chk("sketch.promptDriven", "Prompt to set driven state", true), live("sketch.drivenDefault", "Set driven by default", true)),
      chk("sketch.autoSolveLimit", "Turn off Automatic Solve Mode and Undo when sketch contains more than this number of sketch entities:", true),
      { t: "number", key: "sketch.autoSolveCount", label: "", def: 2000 },
      chk("sketch.hideMissingErr", "Hide missing geometry error for suppressed dimensions", false),
    ],
    children: [
      {
        id: "relationsSnaps",
        label: "Relations/Snaps",
        items: [
          live("sketch.enableSnapping", "Enable snapping", true),
          { t: "button", label: "Go To Document Grid Settings", action: "page:doc.grid" },
          chk("sketch.snapModel", "Snap to model geometry", true),
          live("sketch.automaticRelations", "Automatic relations", true),
          group(
            "Sketch snaps",
            live("sketch.snaps.endpoints", "End points and sketch points", true),
            live("sketch.snaps.centers", "Center Points", true),
            live("sketch.snaps.midpoints", "Mid-points", true),
            live("sketch.snaps.quadrants", "Quadrant Points", true),
            live("sketch.snaps.intersections", "Intersections", true),
            live("sketch.snaps.nearest", "Nearest", true),
            live("sketch.snaps.tangent", "Tangent", true),
            live("sketch.snaps.perpendicular", "Perpendicular", true),
            live("sketch.snaps.parallel", "Parallel", true),
            live("sketch.snaps.hvLines", "Horizontal/vertical lines", true),
            live("sketch.snaps.hvPoints", "Horizontal/vertical to points", true),
            chk("sketch.snaps.length", "Length", true),
            live("sketch.snaps.grid", "Grid", false),
            chk("sketch.snapOnlyGridShown", "Snap only when grid is displayed", true, { indent: 1 }),
            chk("sketch.snaps.angle", "Angle", true),
            { t: "number", key: "sketch.snapAngle", label: "Snap angle:", def: 45, unit: "deg", live: true },
          ),
        ],
      },
    ],
  },
  {
    id: "display",
    label: "Display",
    items: [
      group("Hidden edges displayed as", { t: "radio", key: "display.hiddenEdges", options: [["solid", "Solid"], ["dashed", "Dashed"]], def: "dashed", live: true }),
      group("Part/Assembly tangent edge display", { t: "radio", key: "display.tangentEdges", options: [["visible", "As visible"], ["phantom", "As phantom"], ["removed", "Removed"]], def: "visible" }),
      group(
        "Edge display in shaded with edges mode",
        { t: "radio", key: "display.shadedEdgeMode", options: [["hlr", "HLR"], ["wireframe", "Wireframe"]], def: "hlr" },
        chk("display.thinParts", "Optimize for thin parts", true, { indent: 1 }),
      ),
      group(
        "Assembly transparency for in context edit",
        { t: "select", key: "display.asmTransparency", label: "", options: [["force", "Force assembly transparency"], ["maintain", "Maintain assembly transparency"], ["opaque", "Opaque assembly"]], def: "force" },
        { t: "slider", key: "display.asmTransparencyAmount", label: "", def: 90, min: 0, max: 100, step: 1, left: "0%", right: "100%" },
      ),
      group("Anti-aliasing", { t: "radio", key: "display.antialias", options: [["none", "None"], ["edges", "Anti-alias edges/sketches only"], ["full", "Full scene anti-aliasing"]], def: "full" }),
      group("Appearance Visual Style", { t: "radio", key: "display.visualStyle", options: [["pbr", "Physically Based Rendering"], ["legacy", "Legacy Appearances"]], def: "pbr" }),
      live("display.shadedFaceHighlight", "Use shaded face highlighting", true),
      chk("display.highlightAllEdges", "Highlight all edges of features selected in graphics view", false),
      live("display.dynamicHighlight", "Dynamic highlight from graphics view", true),
      chk("display.tempAxes", "Display temporary axes upon hover over cylindrical faces", false),
      chk("display.openEdges", "Show open edges of surfaces in different color", true),
      live("display.shadedPlanes", "Display shaded planes", true),
      chk("display.dimsFlat", "Display dimensions flat to screen", false),
      chk("display.dimXpertTop", "Display DimXpert dimensions on top of model", false),
      chk("display.notesFlat", "Display notes flat to screen", true),
      live("display.triad", "Display reference triad", true),
      chk("display.scrollbars", "Display scrollbars in graphics view for parts and assemblies", false, { disabled: true }),
      chk("display.drawingScrollbars", "Display scrollbars in graphics view for drawings", true, { disabled: true }),
      chk("display.draftAO", "Display draft quality ambient occlusion", false, { disabled: true }),
      { t: "slider", key: "display.speedpak", label: "Display SpeedPak graphics circle", def: 100, min: 0, max: 100, step: 10, left: "Off", right: "100%" },
      chk("display.patternTooltips", "Display pattern information tooltips", true),
      group("Breadcrumbs", live("display.breadcrumbs", "Show breadcrumbs on selection", true), chk("display.breadcrumbsAtPointer", "Show breadcrumbs at mouse pointer", false, { indent: 1 })),
      chk("display.equationIds", "Display unique equation identifier", true),
      chk("display.facetFins", "Display facet fins in Mesh BREP bodies", true),
      { t: "select", key: "display.fourViewProjection", label: "Projection type for four view viewport:", options: [["first", "First Angle"], ["third", "Third Angle"]], def: "first" },
    ],
  },
  {
    id: "selection",
    label: "Selection",
    items: [
      group("Default bulk selection method", { t: "radio", key: "selection.bulk", options: [["lasso", "Lasso"], ["box", "Box"]], def: "box" }),
      group(
        "Selection of hidden edges",
        chk("selection.hiddenWireframe", "Allow selection in wireframe and HLV modes", true),
        chk("selection.hiddenShaded", "Allow selection in HLR and shaded modes", false),
      ),
      chk("selection.throughTransparency", "Enable selection through transparency", true),
      chk("selection.smallFaces", "Enhance small face selection precision", true),
    ],
  },
  {
    id: "performance",
    label: "Performance",
    items: [
      chk("performance.verify", "Verification on rebuild (enable advanced body checking)", false),
      chk("performance.ignoreSelfIntersect", "Ignore self-intersection check for some sheet metal features", false),
      group("Transparency", chk("performance.transpNormal", "High quality for normal view mode", true), chk("performance.transpDynamic", "High quality for dynamic view mode", true)),
      { t: "select", key: "performance.curvature", label: "Curvature generation:", options: [["demand", "Only on demand"], ["always", "Always (for every shaded model)"]], def: "demand" },
      { t: "slider", key: "performance.lod", label: "Level of detail:", def: 2, min: 0, max: 10, step: 1, left: "More (slower)", right: "Less (faster)" },
      group(
        "Assembly loading",
        { t: "radio", key: "performance.asmMode", options: [["auto", "Automatically optimize resolved mode, hide lightweight mode"], ["manual", "Manually manage resolved and lightweight modes"]], def: "manual" },
        chk("performance.lightweight", "Load component lightweight", false),
        chk("performance.resolveSub", "Always resolve subassemblies", false),
        { t: "select", key: "performance.outOfDate", label: "Check out-of-date lightweight components:", options: [["resolve", "Always Resolve"], ["prompt", "Prompt"], ["dont", "Don't Check"]], def: "resolve" },
        { t: "select", key: "performance.resolveLw", label: "Resolve lightweight components:", options: [["prompt", "Prompt"], ["always", "Always"]], def: "prompt" },
        { t: "select", key: "performance.rebuildOnLoad", label: "Rebuild assembly on load:", options: [["prompt", "Prompt"], ["always", "Always"], ["never", "Never"]], def: "prompt" },
      ),
      group(
        "Mates",
        slider("performance.mateAnim", "Mate animation speed:", 7, "Fast", "Slow"),
        slider("performance.smartMate", "SmartMate sensitivity:", 0, "Fast", "Slow"),
        slider("performance.magneticProx", "Magnetic mate proximity:", 7, "Less", "More"),
        chk("performance.magneticPrealign", "Magnetic mate pre-alignment", true),
      ),
      group(
        "Save",
        chk("performance.purgeConfig", "Purge cached configuration data", false),
        chk("performance.massOnSave", "Update mass properties while saving document", false),
        { t: "select", key: "performance.markModified", label: "Mark assembly as modified when cosmetic changes are made to referenced documents:", options: [["always", "Always"], ["never", "Never"]], def: "always" },
      ),
      chk("performance.shadedPreview", "Use shaded preview", true),
      chk("performance.softwareGL", "Use software OpenGL", false, { disabled: true }),
      { t: "button", label: "Go To Image Quality", action: "page:doc.imageQuality" },
      chk("performance.enhancedGraphics", "Enhanced graphics performance", true),
      chk("performance.hwSilhouette", "Hardware accelerated silhouette edges", true),
      chk("performance.limitSilhouette", "Do not display silhouette edges in parts when the number of bodies exceeds:", true),
      { t: "number", key: "performance.silhouetteBodies", label: "", def: 1000 },
      { t: "select", key: "performance.pixelRatio", label: "Render resolution (OpenCAD):", options: [["1", "Full (best quality)"], ["0.75", "75%"], ["0.5", "50% (fastest)"]], def: "1", live: true },
    ],
  },
  {
    id: "assemblies",
    label: "Assemblies",
    items: [
      chk("asm.dragMove", "Move components by dragging", true),
      // 🔍 SOLIDWORKS' default for the Mass Properties dialog's "Include hidden bodies/components".
      chk("asm.massIncludeHidden", "Include hidden components in Mass Properties by default", true),
      chk("asm.optimizePlacement", "Optimize component placement when adding mates", true),
      // G1d: components ease to their solved placement when a mate is previewed or added (🔍 SOLIDWORKS' page, range and default).
      slider("asm.mateAnimation", "Mate animation speed:", 5, "Off", "Fast"),
      chk("asm.misaligned", "Allow creation of misaligned mates", true),
      chk("asm.aiFastener", "Use AI fastener recognition to create SmartMates when inserting components", true),
      chk("asm.saveExternal", "Save new components to external files", false),
      chk("asm.updateGraphics", "Update model graphics when saving files", false),
      chk("asm.ldrUpdate", "Automatically check and update all components in Large Design Review mode", false),
      { t: "select", key: "asm.mateAlign", label: "Change mate alignments on edit:", options: [["always", "Always"], ["prompt", "Prompt"], ["never", "Never"]], def: "always" },
      { t: "select", key: "asm.speedpak", label: "Update out-of-date Speedpak configurations when saving files:", options: [["rebuild", "With Rebuild on Save mark"], ["all", "All"], ["none", "None"]], def: "rebuild" },
      group(
        "Opening a large assembly",
        chk("asm.lwThreshold", "Use Lightweight mode and Large Assembly Settings when the number of components exceeds:", false),
        { t: "number", key: "asm.lwCount", label: "", def: 500 },
        chk("asm.ldrThreshold", "Use Large Design Review mode when the number of components exceeds:", false),
        { t: "number", key: "asm.ldrCount", label: "", def: 5000 },
      ),
      group(
        "Large Assembly Settings",
        chk("asm.la.noRecover", "Do not save auto recover info", false),
        chk("asm.la.noRebuild", "Do not rebuild when switching to assembly window", false),
        chk("asm.la.hideRefs", "Hide all planes, axes, sketches, curves, annotations, etc.", true),
        chk("asm.la.noEdges", "Do not display edges in shaded mode", true),
        chk("asm.la.noPreview", "Do not preview hidden components", false),
      ),
    ],
  },
  {
    id: "externalRefs",
    label: "External References",
    items: [
      chk("xref.multipleContexts", "Allow multiple contexts for parts when editing in assembly", false),
      { t: "select", key: "xref.load", label: "Load referenced documents:", options: [["prompt", "Prompt"], ["all", "All"], ["none", "None"], ["changed", "Changed"]], def: "prompt" },
      chk("xref.memoryOnly", "Load documents in memory only", true, { indent: 1 }),
      group("Search external references in", chk("xref.fileLocations", "Reference Documents specified in File Locations", true), chk("xref.subfolders", "Include sub-folders", false, { indent: 1 }), chk("xref.excludeActive", "Exclude active folders and recent save locations", false, { indent: 1 })),
      group(
        "Assemblies",
        chk("xref.autoNames", "Automatically generate names for referenced geometry", false),
        chk("xref.allowExternal", "Allow creation of references external to the model", true),
        { t: "radio", key: "xref.componentType", label: "Reference component type:", options: [["any", "Any component"], ["envelope", "Only envelope component"]], def: "any" },
        { t: "radio", key: "xref.context", label: "In the context of:", options: [["top", "Top level assembly"], ["same", "Same subassembly"]], def: "top" },
      ),
      chk("xref.showX", 'Show "x" in feature tree for broken external references', true),
      chk("xref.forceVersion", "Force referenced document to save to the same version as the parent document.", false),
    ],
  },
  {
    id: "templates",
    label: "Default Templates",
    items: [
      { t: "note", text: "These templates are used for operations where OpenCAD does not prompt for a template." },
      { t: "text", key: "templates.part", label: "Parts:", def: "Part.ocpart (built-in)" },
      { t: "text", key: "templates.assembly", label: "Assemblies:", def: "Assembly (built-in)" },
      { t: "text", key: "templates.drawing", label: "Drawings:", def: "Drawing (built-in)" },
      { t: "radio", key: "templates.mode", options: [["always", "Always use these default document templates"], ["prompt", "Prompt user to select document template"]], def: "always" },
    ],
  },
  {
    id: "fileLocations",
    label: "File Locations",
    items: [
      { t: "select", key: "files.show", label: "Show folders for:", options: [["docTemplates", "Document Templates"], ["refDocs", "Referenced Documents"], ["designLibrary", "Design Library"], ["sheetFormats", "Sheet Formats"], ["materials", "Material Databases"], ["appearances", "Appearances"]], def: "docTemplates" },
      { t: "note", text: "In the browser, files are opened and saved through the system file picker. The desktop app will add folder lists here." },
    ],
  },
  {
    id: "featureManager",
    label: "FeatureManager",
    items: [
      chk("fm.scrollIntoView", "Scroll selected item into view", true),
      chk("fm.nameOnCreate", "Name feature on creation", false),
      chk("fm.arrowNav", "Arrow key navigation", false),
      live("fm.dynamicHighlight", "Dynamic highlight", true),
      chk("fm.autoResolve", "Auto-resolve lightweight components upon expansion in FeatureManager tree", true),
      // G1e: where Suppress and a value typed into the Modify box apply in a document with several configurations (🔍 SOLIDWORKS' defaults and page).
      { t: "select", key: "config.suppressScope", label: "Suppress and Unsuppress apply to:", options: [["this", "This configuration"], ["all", "All configurations"]], def: "this" },
      { t: "select", key: "config.modifyScope", label: "Modify box applies a value to:", options: [["this", "This configuration"], ["all", "All configurations"]], def: "this" },
      live("fm.transparentFlyout", "Use transparent flyout FeatureManager tree in parts/assemblies", true),
      live("fm.treeFilter", "Enable FeatureManager tree filter", true),
      chk("fm.previewHidden", "Enable preview of hidden components", true),
      live("fm.slowDoubleClick", "Edit name with slow double click", true),
      chk("fm.translated", "Show translated feature names in FeatureManager tree", false),
      chk("fm.override", "Override document level tree display", false),
      group("Comments", chk("fm.commentTimestamp", "Automatically add timestamp to comments", true), chk("fm.commentsPM", "Show Comments in PropertyManager", true)),
      group(
        "Hide/show tree items",
        ...(
          [
            ["blocks", "Blocks", "auto"],
            ["designBinder", "Design Binder", "auto"],
            ["annotations", "Annotations", "show"],
            ["solidBodies", "Solid Bodies", "auto"],
            ["surfaceBodies", "Surface Bodies", "auto"],
            ["equations", "Equations", "auto"],
            ["material", "Material", "show"],
            ["defaultPlanes", "Default Planes", "show"],
            ["origin", "Origin", "show"],
            ["mateRefs", "Mate References", "auto"],
            ["history", "History", "show"],
            ["sensors", "Sensors", "show"],
          ] as const
        ).map(([k, l, d]): Ctl => ({ t: "select", key: `fm.tree.${k}`, label: l, options: [["auto", "Automatic"], ["show", "Show"], ["hide", "Hide"]], def: d, live: ["annotations", "material", "defaultPlanes", "origin", "history", "sensors"].includes(k) })),
      ),
    ],
  },
  {
    id: "spinBox",
    label: "Spin Box Increments",
    items: [
      group("Length increments", { t: "number", key: "spin.english", label: "English units:", def: 0.1, unit: "in" }, { t: "number", key: "spin.metric", label: "Metric units:", def: 10, unit: "mm", live: true }),
      { t: "number", key: "spin.angle", label: "Angle increments:", def: 1, unit: "deg", live: true },
      { t: "number", key: "spin.time", label: "Time increments:", def: 0.1, unit: "s" },
    ],
  },
  {
    id: "view",
    label: "View",
    items: [
      live("view.reverseWheelZoom", "Reverse mouse wheel zoom direction", false),
      live("view.zoomToFitOnStandard", "Zoom to fit when changing to standard views", false),
      live("view.zoomAboutScreenCenter", "Zoom about screen center (OpenCAD)", false),
      group(
        "View rotation",
        { t: "number", key: "view.arrowKeyIncrement", label: "Arrow keys:", def: 15, unit: "deg", live: true },
        { t: "slider", key: "view.mouseSpeed", label: "Mouse speed:", def: 10, min: 1, max: 10, step: 1, left: "Slow", right: "Fast", live: true },
      ),
      group(
        "Transitions",
        { t: "slider", key: "view.transition", label: "View transition:", def: 10, min: 0, max: 10, step: 1, left: "Off  Slow", right: "Fast", live: true },
        slider("view.hideShowTransition", "Hide/show component:", 10, "Off  Slow", "Fast"),
        slider("view.isolateTransition", "Isolate:", 10, "Off  Slow", "Fast"),
        { t: "slider", key: "view.selectorTransition", label: "View selector:", def: 10, min: 0, max: 10, step: 1, left: "Off  Slow", right: "Fast", live: true },
      ),
    ],
  },
  {
    id: "backup",
    label: "Backup/Recover/Version",
    items: [
      group("Auto-recover", chk("backup.autoRecover", "Save auto-recover information every:", true), { t: "number", key: "backup.minutes", label: "minutes", def: 10, unit: "min" }, { t: "text", key: "backup.folder", label: "Auto-recover folder:", def: "(browser storage)" }),
      group(
        "Backup",
        chk("backup.copies", "Number of backup copies per document:", false),
        { t: "number", key: "backup.count", label: "", def: 1 },
        { t: "radio", key: "backup.where", options: [["folder", "Backup folder"], ["same", "Save backup files in the same location as the original"]], def: "folder" },
        chk("backup.removeOld", "Remove backups older than (days)", false),
        { t: "number", key: "backup.days", label: "", def: 7 },
      ),
      group("Save to Version", chk("backup.saveToVersion", "Enable Save to Version functionality", false)),
    ],
  },
  { id: "touch", label: "Touch", items: [chk("touch.enable", "Enable touch mode", false), chk("touch.gestures", "Enable touch gestures", true)] },
  { id: "holeWizard", label: "Hole Wizard/Toolbox", items: [{ t: "text", key: "hole.folder", label: "Hole Wizard and Toolbox folder:", def: "(built-in standards)" }, chk("hole.toolboxTab", "Make this folder the default search location for Toolbox components", true)] },
  { id: "fileExplorer", label: "File Explorer", items: [chk("fe.myDocs", "My Documents", true), chk("fe.myComputer", "My Computer", true), chk("fe.network", "My Network Places", true), chk("fe.recent", "Recent Documents", true), chk("fe.hidden", "Show hidden referenced documents", false), chk("fe.samples", "Samples", true)] },
  {
    id: "messages",
    label: "Messages/Errors/Warnings",
    items: [
      live("messages.rebuildErrors", "Show errors every rebuild", true),
      chk("messages.warnBeforeSave", "Warn before saving documents with rebuild errors", true),
      chk("messages.warnLargeAsm", "Warn starting a sketch in the context of a large assembly", true),
      chk("messages.dismissTimes", "Dismiss messages after (seconds):", true),
      { t: "number", key: "messages.seconds", label: "", def: 5, unit: "s", live: true },
      group("Display FeatureManager tree warnings", { t: "select", key: "messages.treeWarnings", label: "", options: [["always", "Always"], ["never", "Never"], ["top", "All but top-level"]], def: "always" }),
    ],
    children: [{ id: "dismissed", label: "Dismissed Messages", items: [{ t: "note", text: "Messages you chose not to show again appear here." }] }],
  },
  { id: "sync", label: "Synchronize Settings", items: [chk("sync.enable", "Synchronize settings across devices", false), { t: "note", text: "Settings are stored per browser profile. Export/import of settings files is planned." }] },
  {
    id: "import",
    label: "Import",
    items: [
      { t: "select", key: "import.format", label: "File Format:", options: [["general", "General"], ["step", "STEP/IGES/ACIS"], ["stl", "STL/OBJ/OFF/PLY/PLY2"]], def: "general" },
      group("Solid & Surface", chk("import.tryForm", "Try forming solid(s)", true), chk("import.knitSurfaces", "B-rep mapping", false), chk("import.mergeEntities", "Merge entities", true)),
      { t: "radio", key: "import.unit", label: "Unit", options: [["file", "File specified unit"], ["document", "Document template specified unit"]], def: "file" },
    ],
  },
  {
    id: "export",
    label: "Export",
    items: [
      { t: "select", key: "export.format", label: "File Format:", options: [["step", "STEP"], ["stl", "STL"], ["iges", "IGES"]], def: "step" },
      { t: "radio", key: "export.stepSchema", label: "STEP", options: [["203", "AP203"], ["214", "AP214"], ["242", "AP242"]], def: "214" },
      { t: "radio", key: "export.stlFormat", label: "STL output as", options: [["binary", "Binary"], ["ascii", "ASCII"]], def: "binary" },
      { t: "radio", key: "export.stlResolution", label: "Resolution", options: [["coarse", "Coarse"], ["fine", "Fine"], ["custom", "Custom"]], def: "fine" },
    ],
  },
];

// ------------------------------------------------------------------ Document Properties

export const DOCUMENT_PAGES: Page[] = [
  {
    id: "doc.drafting",
    label: "Drafting Standard",
    items: [
      { t: "select", key: "drafting.standard", label: "Overall drafting standard", options: [["ISO", "ISO"], ["ANSI", "ANSI"], ["DIN", "DIN"], ["JIS", "JIS"], ["BSI", "BSI"], ["GOST", "GOST"], ["GB", "GB"]], def: "ISO" },
      group(
        "Uppercase",
        chk("drafting.upperNotes", "All uppercase for notes", false),
        chk("drafting.upperTables", "All uppercase for tables", false),
        chk("drafting.upperDims", "All uppercase for dimensions and hole callouts", false),
        { t: "text", key: "drafting.exclusions", label: "Exclusion list", def: "m;mm;cm;km;um;µm;nm;s;Hz;Pa;" },
      ),
    ],
    children: [
      {
        id: "doc.annotations",
        label: "Annotations",
        items: [
          { t: "select", key: "ann.baseStandard", label: "Base annotation standard:", options: [["ISO", "ISO"], ["ANSI", "ANSI"]], def: "ISO" },
          group("Leader style", { t: "number", key: "ann.leaderThickness", label: "Thickness:", def: 0.18, unit: "mm" }),
          group("Text", { t: "text", key: "ann.font", label: "Font:", def: "Century Gothic 3.5mm" }),
          { t: "select", key: "ann.attachments", label: "Attachments: Edge/vertex", options: [["filledDot", "Filled dot"], ["arrow", "Closed filled arrow"]], def: "arrow" },
        ],
      },
      {
        id: "doc.dimensions",
        label: "Dimensions",
        items: [
          { t: "select", key: "dim.baseStandard", label: "Base dimension standard:", options: [["ISO", "ISO"], ["ANSI", "ANSI"]], def: "ISO" },
          group("Text", { t: "text", key: "dim.font", label: "Font:", def: "Century Gothic 3.5mm", }),
          group("Primary precision", { t: "select", key: "dim.precision", label: "Length", options: [["0", "None"], ["1", ".1"], ["2", ".12"], ["3", ".123"], ["4", ".1234"]], def: "2", live: true }, { t: "select", key: "dim.anglePrecision", label: "Angle", options: [["0", "None"], ["1", ".1"], ["2", ".12"]], def: "2", live: true }),
          group(
            "Arrows",
            { t: "select", key: "dim.arrowStyle", label: "Style", options: [["filled", "Closed filled"], ["open", "Open"], ["dot", "Dot"]], def: "filled" },
            { t: "number", key: "dim.arrowSize", label: "Length:", def: 3, unit: "mm" },
          ),
          chk("dim.dualDims", "Dual dimensions display", false),
          chk("dim.addParens", "Add parentheses by default (driven dimensions)", true, { live: true }),
          chk("dim.snapText", "Snap text to center", true),
          { t: "select", key: "dim.zeros", label: "Trailing zeroes (dimensions):", options: [["smart", "Smart"], ["show", "Show"], ["remove", "Remove"], ["standard", "Standard"]], def: "show", live: true },
        ],
      },
      { id: "doc.virtualSharps", label: "Virtual Sharps", items: [{ t: "radio", key: "vs.style", options: [["plus", "Plus"], ["star", "Star"], ["dot", "Dot"], ["witness", "Witness lines"]], def: "plus" }] },
      { id: "doc.tables", label: "Tables", items: [{ t: "note", text: "Table settings apply to drawings (bill of materials, hole tables, revision tables)." }, { t: "select", key: "tables.border", label: "Border:", options: [["thin", "0.18mm"], ["thick", "0.35mm"]], def: "thin" }] },
      { id: "doc.dimXpert", label: "DimXpert", items: [{ t: "select", key: "dimxpert.method", label: "Methods:", options: [["block", "Block tolerance"], ["general", "General tolerance"]], def: "general" }] },
    ],
  },
  {
    id: "doc.detailing",
    label: "Detailing",
    items: [
      group("Display filter", chk("detail.cosThreads", "Cosmetic threads", false), chk("detail.shadedCosThreads", "Shaded cosmetic threads", false), chk("detail.datums", "Datums", true), chk("detail.notes", "Notes", true), chk("detail.surfaceFinish", "Surface finishes", true), chk("detail.welds", "Welds", true)),
      chk("detail.annotationsAll", "Display all types", false),
      chk("detail.annotationsFeatureDims", "Display feature dimensions", false),
      chk("detail.refDims", "Display reference dimensions", true),
      { t: "number", key: "detail.textScale", label: "Text scale:", def: 1 },
    ],
  },
  {
    id: "doc.grid",
    label: "Grid/Snap",
    items: [
      group(
        "Grid",
        { t: "check", key: "grid.display", label: "Display grid", def: false, live: true },
        { t: "check", key: "grid.dash", label: "Dash", def: true, live: true },
        { t: "check", key: "grid.autoScale", label: "Automatic scaling", def: true, live: true },
        { t: "number", key: "grid.major", label: "Major grid spacing:", def: 100, unit: "mm", live: true },
        { t: "number", key: "grid.minorPerMajor", label: "Minor-lines per major:", def: 4, live: true },
        { t: "number", key: "grid.snapPerMinor", label: "Snap points per minor:", def: 1, live: true },
      ),
      { t: "button", label: "Go To System Snaps", action: "page:relationsSnaps" },
    ],
  },
  { id: "doc.units", label: "Units", items: [{ t: "unitsTable" }] },
  { id: "doc.modelDisplay", label: "Model Display", items: [{ t: "featureColors" }] },
  {
    id: "doc.material",
    label: "Material Properties",
    items: [
      { t: "number", key: "material.density", label: "Density:", def: 1000, unit: "kg/m^3", live: true },
      group("Area hatch/fill", { t: "radio", key: "material.hatch", options: [["none", "None"], ["solid", "Solid"], ["hatch", "Hatch"]], def: "hatch" }),
    ],
  },
  {
    id: "doc.imageQuality",
    label: "Image Quality",
    items: [
      group(
        "Shaded and draft quality HLR/HLV resolution",
        { t: "slider", key: "iq.shaded", label: "", def: 3, min: 0, max: 10, step: 1, left: "Low (faster)", right: "High (slower)", live: true },
        chk("iq.optimizeEdgeLength", "Optimize edge length (higher quality, but slower)", false),
        chk("iq.saveTess", "Save tessellation with part document", true),
      ),
      group(
        "Wireframe and high quality HLR/HLV resolution",
        { t: "slider", key: "iq.wireframe", label: "", def: 5, min: 0, max: 10, step: 1, left: "Low (faster)", right: "High (slower)", live: true },
        chk("iq.overlap", "Precisely render overlapping geometry (higher quality, but slower)", true, { disabled: true }),
        chk("iq.curveQuality", "Improve curve quality at higher settings", false),
      ),
      chk("iq.isoPreview", "Use isometric, zoom to fit view for document preview", true),
      chk("iq.pre2009", "Use pre-2009 tangent edge definition", false),
    ],
  },
  { id: "doc.sheetMetalMBD", label: "Sheet Metal MBD", items: [{ t: "note", text: "Sheet metal is planned (see docs/ROADMAP.md)." }] },
  { id: "doc.sheetMetal", label: "Sheet Metal", items: [{ t: "note", text: "Sheet metal is planned (see docs/ROADMAP.md)." }] },
  { id: "doc.weldments", label: "Weldments", items: [{ t: "note", text: "Weldments are planned (see docs/ROADMAP.md)." }] },
  {
    id: "doc.planeDisplay",
    label: "Plane Display",
    items: [
      group(
        "Faces",
        { t: "color", key: "plane.front", label: "Front Face Color...", def: "#c6d6ee", live: true },
        { t: "color", key: "plane.back", label: "Back Face Color...", def: "#e8dcc2", live: true },
        { t: "slider", key: "plane.transparency", label: "Transparency:", def: 82, min: 0, max: 100, step: 1, left: "0%", right: "100%", live: true },
      ),
      group("Intersections", { t: "check", key: "plane.intersections", label: "Show intersections", def: true }, { t: "color", key: "plane.intersectionColor", label: "Line Color...", def: "#6b8fc9" }),
    ],
  },
  { id: "doc.configurations", label: "Configurations", items: [chk("config.addRebuild", "Add Rebuild/Save Mark by default", false), chk("config.showLinked", "Show link status in Configuration Manager", true)] },
];

/** Colour scheme entries (System Options > Colors), in SOLIDWORKS order. */
export const COLOR_ITEMS: { key: string; label: string; def: string; live?: string }[] = [
  { key: "selected1", label: "Selected Item 1", def: "#4f9cff", live: "selected" },
  { key: "selected2", label: "Selected Item 2", def: "#ff8ad0" },
  { key: "selected3", label: "Selected Item 3", def: "#9b00ff" },
  { key: "selected4", label: "Selected Item 4", def: "#1ec81e" },
  { key: "selected5", label: "Selected Item 5", def: "#00806e" },
  { key: "selected6", label: "Selected Item 6", def: "#a0005a" },
  { key: "viewportBg", label: "Viewport Background", def: "#c8cdd4", live: "background.1" },
  { key: "topGradient", label: "Top Gradient Color", def: "#ffffff", live: "background.0" },
  { key: "bottomGradient", label: "Bottom Gradient Color", def: "#c8cdd4", live: "background.1" },
  { key: "dynamicHighlight", label: "Dynamic Highlight", def: "#ff9a1f", live: "hover" },
  { key: "highlight", label: "Highlight", def: "#00a2ff" },
  { key: "measureHighlight", label: "Measure Highlight", def: "#ff00ff" },
  { key: "missingRef", label: "Selected Item Missing Reference", def: "#ff0000" },
  { key: "selectedFaceShaded", label: "Selected Face, Shaded", def: "#4f9cff" },
  { key: "drwPaper", label: "Drawings, Paper Color", def: "#ffffff" },
  { key: "drwBackground", label: "Drawings, Background", def: "#c8c8c8" },
  { key: "drwVisible", label: "Drawings, Visible Model Edges", def: "#000000" },
  { key: "drwHidden", label: "Drawings, Hidden Model Edges", def: "#000000" },
  { key: "drwSpeedpak", label: "Drawings, Model Edges (SpeedPak)", def: "#808080" },
  { key: "drwTangent", label: "Drawings, Model Tangent Edges", def: "#808080" },
  { key: "drwChanged", label: "Drawings, Changed dimensions", def: "#0000ff" },
  { key: "drwBomCells", label: "Drawings, Modified Cells (BOM)", def: "#ff8000" },
  { key: "drwOverridden", label: "Drawings, Overriden Dimensions", def: "#0000ff" },
  { key: "dimImported", label: "Dimensions, Imported (Driving)", def: "#000000", live: "dimension" },
  { key: "dimDriven", label: "Dimensions, Non Imported (Driven)", def: "#7a7a7a", live: "dimensionDriven" },
  { key: "dimDangling", label: "Dimensions, Dangling", def: "#9a7a00" },
  { key: "dimPreview", label: "Dimensions, Preview", def: "#8080ff" },
  { key: "dimNotMarked", label: "Dimensions, Not Marked for Drawing", def: "#a000a0" },
  { key: "dimDesignTable", label: "Dimensions, Controlled by Design Table", def: "#ff00ff" },
  { key: "dimVariability", label: "Dimensions, Controlled by Variability Table", def: "#202020" },
  { key: "graphicalAnnotations", label: "Graphical Annotations", def: "#202020" },
  { key: "text", label: "Text", def: "#000000" },
  { key: "sketchOver", label: "Sketch, Over Defined", def: "#ff0000", live: "sketchOver" },
  { key: "sketchFull", label: "Sketch, Fully Defined", def: "#000000", live: "sketchFull" },
  { key: "sketchUnder", label: "Sketch, Under Defined", def: "#0000ff", live: "sketchUnder" },
  { key: "sketchInvalid", label: "Sketch, Invalid Geometry", def: "#c8c800" },
  { key: "sketchNotSolved", label: "Sketch, Not Solved", def: "#ff8080" },
  { key: "sketchInactive", label: "Sketch, Inactive", def: "#5a5a5a", live: "sketchInactive" },
  { key: "sketchShaded", label: "Sketch, Shaded Contours", def: "#b4c8ff" },
  { key: "sketchExploded", label: "Sketch, Exploded", def: "#000000" },
  { key: "gridMinor", label: "Grid Lines, Minor", def: "#c8c8c8" },
  { key: "gridMajor", label: "Grid Lines, Major", def: "#969696" },
  { key: "construction", label: "Construction Geometry", def: "#0000ff" },
  { key: "asmEditPart", label: "Assembly, Edit Part", def: "#0000ff" },
  { key: "asmHiddenEdit", label: "Assembly, Hidden Lines of Edit Part", def: "#0000a0" },
  { key: "asmNonEdit", label: "Assembly, Non-Edit Parts", def: "#a0a0a0" },
  { key: "inactiveEntities", label: "Inactive Entities", def: "#a0a0a0" },
  { key: "inactiveHandles", label: "Inactive Handles", def: "#808080" },
  { key: "tempGraphics", label: "Temporary Graphics", def: "#ffff00" },
  { key: "tempShaded", label: "Temporary Graphics, Shaded", def: "#ffff80" },
  { key: "tempAdd", label: "Temporary Graphics, Add Material", def: "#00ff00" },
  { key: "tempRemove", label: "Temporary Graphics, Remove Material", def: "#ff0000" },
  { key: "smTemp", label: "Sheet Metal Temporary Graphics Color", def: "#ff8000" },
  { key: "surfacesOpenEdges", label: "Surfaces, Open Edges", def: "#0080c0" },
  { key: "edgesShaded", label: "Edges in Shaded With Edges Mode", def: "#1a1a1a", live: "edges" },
  { key: "fmText", label: "FeatureManager Design Tree Text", def: "#000000" },
  { key: "flyoutText", label: "Flyout FeatureManager Design Tree Text", def: "#000000" },
  { key: "annotationsImported", label: "Annotations, Imported", def: "#000000" },
  { key: "annotationsNotImported", label: "Annotations, Non Imported", def: "#000000" },
  { key: "interference", label: "Assembly Interference Volume", def: "#ff0000" },
  { key: "hiddenEdgeSelection", label: "Hidden Edge Selection Show Color", def: "#ff80ff" },
  { key: "mateHealthy", label: "Mate Callout, Healthy", def: "#80ff80" },
  { key: "mateWarning", label: "Mate Callout, Warning", def: "#ffff80" },
  { key: "mateError", label: "Mate Callout, Error", def: "#ff8080" },
  { key: "envelope", label: "Envelope Components", def: "#80ffff" },
  { key: "asmVis1", label: "Assembly Visualization First", def: "#0000ff" },
  { key: "asmVis2", label: "Assembly Visualization Second", def: "#00c0ff" },
  { key: "asmVis3", label: "Assembly Visualization Third", def: "#00ff00" },
  { key: "asmVis4", label: "Assembly Visualization Fourth", def: "#ffff00" },
  { key: "asmVis5", label: "Assembly Visualization Fifth", def: "#ff8000" },
  { key: "asmVis6", label: "Assembly Visualization Sixth", def: "#ff0000" },
  { key: "manipulator", label: "Manipulator connection point color", def: "#40a0c0" },
  { key: "dimXpertAnn", label: "Annotations, DimXpert", def: "#408080" },
  { key: "dimXpertUnder", label: "DimXpert, Under Constrained", def: "#ffff00" },
  { key: "dimXpertFull", label: "DimXpert, Fully Constrained", def: "#00c000" },
  { key: "dimXpertOver", label: "DimXpert, Over Constrained", def: "#ff0000" },
  { key: "tolAnalyst", label: "Annotations, TolAnalyst Dimension", def: "#ff00ff" },
  { key: "zoneRow", label: "Zone row/column lines", def: "#ff80c0" },
  { key: "meshRecognized", label: "Recognized mesh face", def: "#00c000" },
  { key: "meshUnrecognized", label: "Unrecognized mesh face", def: "#c00000" },
  { key: "inferenceRelation", label: "Inference Line, Relation (OpenCAD)", def: "#1f5fff", live: "inferenceRelation" },
  { key: "inferenceGuide", label: "Inference Line, Guide (OpenCAD)", def: "#e8a200", live: "inferenceGuide" },
];

/** Model/feature colours (Document Properties > Model Display). */
export const FEATURE_COLOR_ITEMS: { key: string; label: string; def: string; live?: boolean }[] = [
  { key: "wireframe", label: "Wireframe/HLR", def: "#000000" },
  { key: "shading", label: "Shading", def: "#c9ccd1", live: true },
  { key: "hidden", label: "Hidden", def: "#808080", live: true },
  { key: "cartoon", label: "Cartoon edges", def: "#000000" },
  { key: "bend", label: "Bend", def: "#c9ccd1" },
  { key: "boss", label: "Boss", def: "#c9ccd1" },
  { key: "cavity", label: "Cavity", def: "#c9ccd1" },
  { key: "chamfer", label: "Chamfer", def: "#c9ccd1" },
  { key: "cut", label: "Cut", def: "#c9ccd1" },
  { key: "cutLoft", label: "Cut-Loft", def: "#c9ccd1" },
  { key: "cutSurface", label: "Cut-Surface", def: "#c9ccd1" },
  { key: "cutSweep", label: "Cut-Sweep", def: "#c9ccd1" },
  { key: "extrude", label: "Extrude", def: "#c9ccd1" },
  { key: "fillet", label: "Fillet", def: "#c9ccd1" },
  { key: "hole", label: "Hole", def: "#c9ccd1" },
  { key: "loft", label: "Loft", def: "#c9ccd1" },
  { key: "pattern", label: "Pattern", def: "#c9ccd1" },
  { key: "revolve", label: "Revolve", def: "#c9ccd1" },
  { key: "shell", label: "Shell", def: "#c9ccd1" },
  { key: "sweep", label: "Sweep", def: "#c9ccd1" },
];

/** Lookup of every control by key, for reading defaults. */
export function collectDefaults(pages: Page[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  const walk = (items: Ctl[]) => {
    for (const c of items) {
      if (c.t === "group") walk(c.items);
      else if ("key" in c && "def" in c) out[c.key] = c.def;
    }
  };
  const visit = (ps: Page[]) => ps.forEach((p) => (walk(p.items), p.children && visit(p.children)));
  visit(pages);
  return out;
}

export const DOC_DEFAULTS = collectDefaults(DOCUMENT_PAGES);
