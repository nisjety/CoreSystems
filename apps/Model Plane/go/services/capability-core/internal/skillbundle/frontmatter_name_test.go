package skillbundle

import "testing"

// FrontMatterName reads the name the way Parse checks it: a column-zero key inside the block
// at the top of SKILL.md, trimmed. Any other file, or a body line, never supplies it.
func TestFrontMatterNameReadsTheSkillFile(t *testing.T) {
	bundle := Bundle{Files: []File{
		{Path: "references/name.md", Body: []byte("---\nname: other\n---\n")},
		{Path: SkillFileName, Body: []byte("---\r\nname:  Weekly report \r\ndescription: d\r\n---\r\n  name: body\r\n")},
	}}
	if got := FrontMatterName(bundle); got != "Weekly report" {
		t.Fatalf("FrontMatterName = %q, want the SKILL.md front matter name", got)
	}
	if got := FrontMatterName(Bundle{Files: []File{{Path: SkillFileName, Body: []byte("no front matter")}}}); got != "" {
		t.Fatalf("FrontMatterName without front matter = %q, want empty", got)
	}
}

// The refactor behind FrontMatterName keeps Parse's rule: the block must close and name both keys.
func TestValidFrontMatterStillNeedsAClosedBlockWithNameAndDescription(t *testing.T) {
	for body, want := range map[string]bool{
		"---\nname: a\ndescription: b\n---\n": true,
		"---\nname: a\ndescription: b\n":      false,
		"---\nname: a\n---\n":                 false,
		" name: a\n---\n":                     false,
	} {
		if got := validFrontMatter([]byte(body)); got != want {
			t.Errorf("validFrontMatter(%q) = %v, want %v", body, got, want)
		}
	}
}
