def safe_message(value: object) -> str:
    rendered = []
    for character in str(value):
        if not character.isprintable() or character in "<>&":
            rendered.append(f"\\u{{{ord(character):x}}}")
        else:
            rendered.append(character)
    return "".join(rendered)
