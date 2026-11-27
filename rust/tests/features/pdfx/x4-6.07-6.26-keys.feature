Feature: PDF/X-4 6.7 to 6.26, keys forbidden anywhere in the file
  No external content or unembedded file specifications, no transfer functions, halftone phases or named or threshold
  halftones, no PostScript, no printed image alternates, no actions or JavaScript, no viewer areas but the media or
  bleed box, no alternate presentations, and no XFA. They hold for the whole file, not only what the pages paint.

  Scenario Outline: One forbidden key raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-xobject-opi | x4.xobject.no-opi |
      | x4-form-ref | x4.xobject.no-ref |
      | x4-stream-external | x4.stream.no-external-f |
      | x4-filespec-not-embedded | x4.filespec.embedded |
      | x4-gstate-tr | x4.extgstate.no-tr |
      | x4-gstate-htp | x4.extgstate.no-htp |
      | x4-gstate-tr2 | x4.extgstate.tr2-default |
      | x4-halftone-type | x4.halftone.type-1-or-5 |
      | x4-halftone-name | x4.halftone.no-name |
      | x4-halftone-transfer | x4.halftone.transfer-function |
      | x4-postscript-xobject | x4.xobject.no-postscript |
      | x4-form-subtype2-ps | x4.xobject.no-subtype2-ps |
      | x4-image-alternate-printed | x4.image.no-default-for-printing |
      | x4-open-action | x4.actions.absent |
      | x4-javascript | x4.javascript.absent |
      | x4-viewer-print-area | x4.viewer-prefs.areas |
      | x4-alternate-presentations | x4.names.no-alternate-presentations |
      | x4-pres-steps | x4.page.no-pres-steps |
      | x4-xfa | x4.acroform.no-xfa |
