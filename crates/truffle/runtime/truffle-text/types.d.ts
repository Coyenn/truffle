// Platform-agnostic Truffle Text layout types (no rendering).

export interface FontMetaPage {
	w: number;
	h: number;
}

export interface FontKerningMeta {
	leftClass: number[];
	rightClass: number[];
	leftCount: number;
	rightCount: number;
	matrix: number[];
}

export interface FontMetaV2 {
	lineHeight: number;
	baseline: number;
	px: number;
	charset: string;
	pages: FontMetaPage[];
	advances: number[];
	offsetX: number[];
	offsetY: number[];
	rects: number[];
	kerning: FontKerningMeta;
}

export interface FontMetaOutlineV2 extends Omit<FontMetaV2, "kerning"> {
	kerning?: FontKerningMeta;
}

export interface LayoutOptions {
	textSize?: number;
	lineHeight?: number;
	letterSpacing?: number;
	lineSpacing?: number;
	maxWidth?: number;
	maxHeight?: number;
	truncate?: boolean;
	alignX?: "left" | "center" | "right";
	alignY?: "top" | "center" | "bottom";
}

export interface LayoutGlyph {
	character: string;
	index: number;
	x: number;
	y: number;
	width: number;
	height: number;
	advance: number;
	atlasPage: number;
	srcX: number;
	srcY: number;
	srcW: number;
	srcH: number;
	offsetX: number;
	offsetY: number;
	attrs: Record<string, unknown>;
}

export interface LayoutResult {
	glyphs: LayoutGlyph[];
	width: number;
	height: number;
	lineCount: number;
	truncated: boolean;
	scale: number;
}

export interface RichTextSpan {
	character: string;
	attrs: Record<string, unknown>;
}

export interface TagHandler {
	onOpen?: (tag: string, attrs: Record<string, string>, state: Record<string, unknown>) => void;
	onClose?: (tag: string, state: Record<string, unknown>) => void;
}

export interface TruffleFontHandle {
	getLineHeight(): number;
	getPx(): number;
	getCharset(): string;
	getAdvance(character: string): number;
	getKerning(left: string, right: string): number;
}
