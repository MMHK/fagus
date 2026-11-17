# Security policy

Fagus renders HTML and CSS that may come from untrusted sources. Security reports are taken seriously.

## Supported versions

Security fixes go into the latest release. Before 1.0 that is the newest minor version (0.2.x at the moment); there are no fixes for older ones.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through GitHub: [report a vulnerability](https://github.com/nforst/fagus/security/advisories/new) (the *Security* tab of the repository). If you cannot use GitHub, send the report to kontakt@niklasforst.de.

Include a description, the affected version and, if possible, a document that reproduces the problem. You will get an answer within a week. Once a fix is released, the report is published as a security advisory, crediting you if you wish.

## Scope

In scope: code execution, reading files or network resources that the caller did not allow, crashes and unbounded resource use caused by input documents, and incorrect output in PDF/A or PDF/UA mode that a validator rejects.
