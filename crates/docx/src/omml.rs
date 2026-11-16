use model::{EquationNode, LimitPosition, VerticalPosition};

pub fn node_to_omml(node: &EquationNode) -> String {
    match node {
        EquationNode::Row { children } => children.iter().map(node_to_omml).collect::<String>(),
        EquationNode::Identifier { value } | EquationNode::Number { value } | EquationNode::Text { value } |
        EquationNode::Symbol { value } | EquationNode::Operator { value } | EquationNode::Relation { value } => math_run(value),
        EquationNode::Fraction { numerator, denominator } => format!(
            "<m:f><m:num>{}</m:num><m:den>{}</m:den></m:f>", node_to_omml(numerator), node_to_omml(denominator)),
        EquationNode::Radical { radicand, index } => {
            let degree = index.as_ref().map(|n| node_to_omml(n)).unwrap_or_default();
            let deg_hide = if index.is_none() { "<m:radPr><m:degHide m:val=\"1\"/></m:radPr>" } else { "" };
            format!("<m:rad>{deg_hide}<m:deg>{degree}</m:deg><m:e>{}</m:e></m:rad>", node_to_omml(radicand))
        }
        EquationNode::Superscript { base, exponent } => format!("<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>", node_to_omml(base), node_to_omml(exponent)),
        EquationNode::Subscript { base, subscript } => format!("<m:sSub><m:e>{}</m:e><m:sub>{}</m:sub></m:sSub>", node_to_omml(base), node_to_omml(subscript)),
        EquationNode::Scripts { base, subscript, superscript } => match (subscript, superscript) {
            (Some(sub), Some(sup)) => format!("<m:sSubSup><m:e>{}</m:e><m:sub>{}</m:sub><m:sup>{}</m:sup></m:sSubSup>", node_to_omml(base), node_to_omml(sub), node_to_omml(sup)),
            (Some(sub), None) => format!("<m:sSub><m:e>{}</m:e><m:sub>{}</m:sub></m:sSub>", node_to_omml(base), node_to_omml(sub)),
            (None, Some(sup)) => format!("<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>", node_to_omml(base), node_to_omml(sup)),
            (None, None) => node_to_omml(base),
        },
        EquationNode::Matrix { rows } => {
            let rows = rows.iter().map(|row| {
                let cells = row.iter().map(|cell| format!("<m:e>{}</m:e>", node_to_omml(cell))).collect::<String>();
                format!("<m:mr>{cells}</m:mr>")
            }).collect::<String>();
            format!("<m:m>{rows}</m:m>")
        }
        EquationNode::Delimited { left, body, right } => format!("<m:d><m:dPr><m:begChr m:val=\"{}\"/><m:endChr m:val=\"{}\"/></m:dPr><m:e>{}</m:e></m:d>", xml_escape(left), xml_escape(right), node_to_omml(body)),
        EquationNode::Function { name, argument } => format!("<m:func><m:fName>{}</m:fName><m:e>{}</m:e></m:func>", math_run(name), node_to_omml(argument)),
        EquationNode::Accent { accent, body } => format!("<m:acc><m:accPr><m:chr m:val=\"{}\"/></m:accPr><m:e>{}</m:e></m:acc>", xml_escape(accent), node_to_omml(body)),
        EquationNode::Nary { operator, body, subscript, superscript } => {
            let sub = subscript.as_ref().map(|n| node_to_omml(n)).unwrap_or_default();
            let sup = superscript.as_ref().map(|n| node_to_omml(n)).unwrap_or_default();
            let sub_hide = if subscript.is_none() { "<m:subHide m:val=\"1\"/>" } else { "" };
            let sup_hide = if superscript.is_none() { "<m:supHide m:val=\"1\"/>" } else { "" };
            format!("<m:nary><m:naryPr><m:chr m:val=\"{}\"/>{sub_hide}{sup_hide}</m:naryPr><m:sub>{sub}</m:sub><m:sup>{sup}</m:sup><m:e>{}</m:e></m:nary>", xml_escape(operator), node_to_omml(body))
        }
        EquationNode::Limit { base, limit, position } => match position {
            LimitPosition::Lower => format!("<m:limLow><m:e>{}</m:e><m:lim>{}</m:lim></m:limLow>", node_to_omml(base), node_to_omml(limit)),
            LimitPosition::Upper => format!("<m:limUpp><m:e>{}</m:e><m:lim>{}</m:lim></m:limUpp>", node_to_omml(base), node_to_omml(limit)),
        },
        EquationNode::PreScripts { base, subscript, superscript } => {
            let sub = subscript.as_ref().map(|n| node_to_omml(n)).unwrap_or_default();
            let sup = superscript.as_ref().map(|n| node_to_omml(n)).unwrap_or_default();
            format!("<m:sPre><m:sub>{sub}</m:sub><m:sup>{sup}</m:sup><m:e>{}</m:e></m:sPre>", node_to_omml(base))
        }
        EquationNode::Bar { body, position } => {
            let pos = match position { VerticalPosition::Over => "top", VerticalPosition::Under => "bot" };
            format!("<m:bar><m:barPr><m:pos m:val=\"{pos}\"/></m:barPr><m:e>{}</m:e></m:bar>", node_to_omml(body))
        }
        EquationNode::GroupCharacter { body, character, position } => {
            let pos = match position { VerticalPosition::Over => "top", VerticalPosition::Under => "bot" };
            format!("<m:groupChr><m:groupChrPr><m:chr m:val=\"{}\"/><m:pos m:val=\"{pos}\"/></m:groupChrPr><m:e>{}</m:e></m:groupChr>", xml_escape(character), node_to_omml(body))
        }
        EquationNode::EquationArray { rows } => {
            let rows = rows.iter().map(|row| format!("<m:e>{}</m:e>", node_to_omml(row))).collect::<String>();
            format!("<m:eqArr>{rows}</m:eqArr>")
        }
        EquationNode::Boxed { body } => format!("<m:borderBox><m:e>{}</m:e></m:borderBox>", node_to_omml(body)),
        EquationNode::Phantom { body } => format!("<m:phant><m:e>{}</m:e></m:phant>", node_to_omml(body)),
        EquationNode::Space { .. } => math_run(" "),
        EquationNode::RawTypst { source } => math_run(source),
    }
}

fn math_run(text: &str) -> String { format!("<m:r><m:t>{}</m:t></m:r>", xml_escape(text)) }
fn xml_escape(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }
