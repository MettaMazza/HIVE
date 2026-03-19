import os
import re

count = 0
for root, dirs, files in os.walk("src"):
    for file in files:
        if file.endswith(".rs"):
            filepath = os.path.join(root, file)
            with open(filepath, "r") as f:
                content = f.read()

            # Find Event definitions like:
            # Event { platform: ..., scope: ..., author_name: ..., author_id: ..., content: ... }
            # We want to replace `content: [expr]` or `content,` with `content: [expr], timestamp: Some(chrono::Utc::now().to_rfc3339()), message_index: None`
            
            # Use regex to find `content: <expr>,` OR `content: <expr> }` OR `content,` and inject our fields.
            
            def replace_content(match):
                global count
                count += 1
                base = match.group(1)
                # Ensure we have a trailing comma before injecting
                if base.strip().endswith(','):
                    return base + "\n            timestamp: Some(chrono::Utc::now().to_rfc3339()),\n            message_index: None,"
                else:
                    return base + ",\n            timestamp: Some(chrono::Utc::now().to_rfc3339()),\n            message_index: None,"

            # Match `content: anything,` or `content: anything \n` or `content,\n` inside Event struct instantiations.
            # This is tricky because content might be a multi-line format! macro.
            # A safer way: just append to the block before `}` if it's an Event block.
            
            def replace_event_block(match):
                global count
                count += 1
                inner = match.group(2)
                # Ensure it ends with a comma (if it has fields)
                if inner.strip() and not inner.strip().endswith(','):
                    inner = inner + ","
                return match.group(1) + "{" + inner + "\n            timestamp: Some(chrono::Utc::now().to_rfc3339()),\n            message_index: None,\n        }"

            new_content = re.sub(r'(Event\s*)\{([^}]*content[^}]*)\}', replace_event_block, content, flags=re.DOTALL)
            
            with open(filepath, "w") as f:
                f.write(new_content)

print(f"Injected fields into {count} locations.")
