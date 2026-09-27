4207eb90: mv	a1, a0
4207eb92: lw	a3, 0x8(a0)
4207eb94: lw	a0, 0x0(a0)
4207eb96: beqz	a3, 0x4207ebcc <<brewthink::bounded_xml::XmlTag>::local_name+0x3c>
4207eb98: lw	a1, 0x4(a1)
4207eb9a: bgeu	a3, a1, 0x4207ebc8 <<brewthink::bounded_xml::XmlTag>::local_name+0x38>
4207eb9e: add	a2, a0, a3
4207eba2: lb	a2, 0x0(a2)
4207eba6: li	a4, -0x41
4207ebaa: blt	a4, a2, 0x4207ebcc <<brewthink::bounded_xml::XmlTag>::local_name+0x3c>
4207ebae: addi	sp, sp, -0x10
4207ebb0: sw	ra, 0xc(sp)
4207ebb2: sw	s0, 0x8(sp)
4207ebb4: addi	s0, sp, 0x10
4207ebb6: lui	a4, 0x3c027
4207ebba: addi	a4, a4, 0xb0
4207ebbe: li	a2, 0x0
4207ebc0: auipc	ra, 0xe
4207ebc4: jalr	0x3a6(ra) <core::str::slice_error_fail>
4207ebc8: bne	a3, a1, 0x4207ebae <<brewthink::bounded_xml::XmlTag>::local_name+0x1e>
4207ebcc: mv	a1, a3
4207ebce: auipc	t1, 0x3
4207ebd2: jr	-0x70a(t1) <brewthink::bounded_xml::local_name>
