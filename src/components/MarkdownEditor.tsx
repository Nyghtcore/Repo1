import { useEffect, useRef } from "react";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { Placeholder } from "@tiptap/extension-placeholder";
import { Markdown } from "@tiptap/markdown";

import { withoutCheckboxState } from "../utils/format";

export interface MarkdownEditorProps {
  /** Markdown body loaded from disk. */
  value: string;
  /** Identifies the document; changing it resets the editor content. */
  documentKey: string;
  /**
   * Called with the serialised markdown whenever the user edits.
   * `immediate` is `true` for checkbox toggles, which must hit the disk at once.
   */
  onChange: (markdown: string, immediate: boolean) => void;
  placeholder?: string;
  readOnly?: boolean;
}

/**
 * Rich-text editor backed by TipTap. Content flows in as markdown
 * (`contentType: "markdown"`) and flows back out through `getMarkdown()`,
 * so the file on disk always stays plain, human-readable markdown.
 */
export function MarkdownEditor({
  value,
  documentKey,
  onChange,
  placeholder,
  readOnly = false,
}: MarkdownEditorProps) {
  const lastEmitted = useRef<string>(value);
  const loadedKey = useRef<string>(documentKey);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3, 4] },
        link: { openOnClick: false, autolink: true },
      }),
      TaskList,
      TaskItem.configure({ nested: true }),
      Placeholder.configure({ placeholder: placeholder ?? "Start writing…" }),
      Markdown.configure({ indentation: { style: "space", size: 2 } }),
    ],
    content: value,
    contentType: "markdown",
    editable: !readOnly,
    editorProps: {
      attributes: { class: "editor-surface", spellcheck: "true" },
    },
    onUpdate: ({ editor: instance }) => {
      const markdown = instance.getMarkdown();
      if (markdown === lastEmitted.current) return;
      // A toggle of `[ ]` <-> `[x]` leaves everything else identical.
      const immediate =
        withoutCheckboxState(markdown) === withoutCheckboxState(lastEmitted.current);
      lastEmitted.current = markdown;
      onChange(markdown, immediate);
    },
  });

  // Load new content when switching documents or when the file changed on
  // disk (e.g. Syncthing delivered a peer's edit).
  useEffect(() => {
    if (!editor) return;
    const switchedDocument = loadedKey.current !== documentKey;
    if (switchedDocument || value !== lastEmitted.current) {
      loadedKey.current = documentKey;
      lastEmitted.current = value;
      editor.commands.setContent(value, { contentType: "markdown", emitUpdate: false });
      if (switchedDocument) editor.commands.focus("start");
    }
  }, [editor, value, documentKey]);

  useEffect(() => {
    editor?.setEditable(!readOnly);
  }, [editor, readOnly]);

  if (!editor) return <div className="editor-loading">Loading editor…</div>;

  return (
    <div className="editor">
      <div className="editor-toolbar" role="toolbar" aria-label="Formatting">
        <ToolbarButton
          label="H1"
          active={editor.isActive("heading", { level: 1 })}
          onClick={() => editor.chain().focus().toggleHeading({ level: 1 }).run()}
        />
        <ToolbarButton
          label="H2"
          active={editor.isActive("heading", { level: 2 })}
          onClick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()}
        />
        <ToolbarButton
          label="B"
          title="Bold"
          active={editor.isActive("bold")}
          onClick={() => editor.chain().focus().toggleBold().run()}
        />
        <ToolbarButton
          label="I"
          title="Italic"
          active={editor.isActive("italic")}
          onClick={() => editor.chain().focus().toggleItalic().run()}
        />
        <ToolbarButton
          label="S"
          title="Strike"
          active={editor.isActive("strike")}
          onClick={() => editor.chain().focus().toggleStrike().run()}
        />
        <ToolbarButton
          label="‹›"
          title="Code"
          active={editor.isActive("code")}
          onClick={() => editor.chain().focus().toggleCode().run()}
        />
        <span className="toolbar-sep" />
        <ToolbarButton
          label="• List"
          active={editor.isActive("bulletList")}
          onClick={() => editor.chain().focus().toggleBulletList().run()}
        />
        <ToolbarButton
          label="1. List"
          active={editor.isActive("orderedList")}
          onClick={() => editor.chain().focus().toggleOrderedList().run()}
        />
        <ToolbarButton
          label="☐ Task"
          active={editor.isActive("taskList")}
          onClick={() => editor.chain().focus().toggleTaskList().run()}
        />
        <ToolbarButton
          label="❝"
          title="Quote"
          active={editor.isActive("blockquote")}
          onClick={() => editor.chain().focus().toggleBlockquote().run()}
        />
        <ToolbarButton
          label="{ }"
          title="Code block"
          active={editor.isActive("codeBlock")}
          onClick={() => editor.chain().focus().toggleCodeBlock().run()}
        />
        <ToolbarButton
          label="—"
          title="Horizontal rule"
          onClick={() => editor.chain().focus().setHorizontalRule().run()}
        />
      </div>
      <EditorContent editor={editor} className="editor-content" />
    </div>
  );
}

function ToolbarButton({
  label,
  title,
  active = false,
  onClick,
}: {
  label: string;
  title?: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`toolbar-btn${active ? " active" : ""}`}
      title={title ?? label}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
    >
      {label}
    </button>
  );
}
