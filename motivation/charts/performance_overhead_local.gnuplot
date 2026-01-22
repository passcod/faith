set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/performance_overhead_local.png'
set title 'Request Overhead (Local Target - minus x1 baseline) (lower is better)'
set xlabel 'Number of Requests'
set ylabel 'Overhead Duration (ms)'
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("10" 0, "100" 1)


plot 'charts/performance_overhead_local_data.txt' using 2:xtic(1) title 'native', \
     '' using 3 title 'node-fetch', \
     '' using 4 title 'Fáith', \
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle
