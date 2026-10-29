package extract

import "testing"

func TestHTMLToText(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name string
		in   string
		want string
	}{
		{name: "empty", in: "", want: ""},
		{name: "whitespace only", in: "  \n\t ", want: ""},
		{name: "plain text passes through", in: "no markup here", want: "no markup here"},
		{
			name: "block tags become newlines",
			in:   "<h2>Title</h2><p>First paragraph.</p><p>Second.</p>",
			want: "Title\nFirst paragraph.\nSecond.",
		},
		{
			name: "inline tags become spaces",
			in:   "<span>one</span><b>two</b>",
			want: "one two",
		},
		{
			name: "entities decoded",
			in:   "<p>Fish &amp; chips &lt;3</p>",
			want: "Fish & chips <3",
		},
		{
			name: "script and style dropped whole",
			in:   "<p>keep</p><script>var x = 'drop';</script><style>.a{color:red}</style><p>tail</p>",
			want: "keep\ntail",
		},
		{
			name: "list items separated",
			in:   "<ul><li>alpha</li><li>beta</li></ul>",
			want: "alpha\nbeta",
		},
		{
			name: "attributes ignored",
			in:   `<div class="hero" data-x="1">content</div>`,
			want: "content",
		},
		{
			name: "unterminated tag drops trailing garbage",
			in:   "text before <p unclosed",
			want: "text before",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if got := HTMLToText(tc.in); got != tc.want {
				t.Fatalf("HTMLToText(%q) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}
